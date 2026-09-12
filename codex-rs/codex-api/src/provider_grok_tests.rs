use crate::common::Reasoning;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::provider::Provider;
use crate::provider::ResponsesDialect;
use crate::provider::RetryConfig;
use codex_protocol::ResponseItemId;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use http::HeaderMap;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

fn provider(name: &str) -> Provider {
    Provider {
        name: name.to_string(),
        base_url: "https://example.test/v1".to_string(),
        query_params: None,
        headers: HeaderMap::new(),
        retry: RetryConfig {
            max_attempts: 1,
            base_delay: Duration::ZERO,
            retry_429: false,
            retry_5xx: false,
            retry_transport: false,
        },
        stream_idle_timeout: Duration::from_secs(1),
    }
}

fn request(input: Vec<ResponseItem>) -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "grok-4.6".to_string(),
        instructions: "test".to_string(),
        input,
        tools: None,
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
        reasoning: Some(Reasoning {
            effort: Some(ReasoningEffort::XHigh),
            summary: None,
            context: None,
        }),
        store: false,
        stream: true,
        stream_options: None,
        include: vec!["reasoning.encrypted_content".to_string()],
        service_tier: None,
        prompt_cache_key: Some("thread".to_string()),
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn responses_dialect_is_selected_only_for_grok_provider_identity() {
    assert_eq!(
        ResponsesDialect::for_provider(&provider("Grok")),
        ResponsesDialect::Grok
    );
    assert_eq!(
        ResponsesDialect::for_provider(&provider("gRoK")),
        ResponsesDialect::Grok
    );
    assert_eq!(
        ResponsesDialect::for_provider(&provider("OpenAI")),
        ResponsesDialect::OpenAi
    );
    assert_eq!(
        ResponsesDialect::for_provider(&provider("Custom")),
        ResponsesDialect::OpenAi
    );
}

#[test]
fn grok_projects_replayed_history_on_request_copy_only() {
    let envelope =
        "Message Type: NEW_TASK\nTask name: /root/child\nSender: /root\nPayload:\nreview";
    let input = vec![
        user_message("start"),
        ResponseItem::AgentMessage {
            id: Some(ResponseItemId::with_suffix("amsg", "child")),
            author: "/root".to_string(),
            recipient: "/root/child".to_string(),
            content: vec![AgentMessageInputContent::InputText {
                text: envelope.to_string(),
            }],
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: Some("notify".to_string()),
            namespace: None,
            output: FunctionCallOutputPayload::from_text("scheduled task fired".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::Reasoning {
            id: Some(ResponseItemId::with_suffix("rs", "reasoning-id")),
            summary: vec![ReasoningItemReasoningSummary::SummaryText {
                text: "summary".to_string(),
            }],
            content: None,
            encrypted_content: Some("opaque-encrypted-reasoning".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
    ];
    let canonical = request(input);
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical)
        .expect("Grok history should project");

    assert_eq!(
        canonical, original,
        "durable/canonical request must not mutate"
    );
    assert_eq!(projected["input"][0]["type"], "message");
    assert_eq!(projected["input"][1]["type"], "message");
    assert_eq!(projected["input"][1]["role"], "user");
    assert_eq!(projected["input"][1]["content"][0]["text"], envelope);
    assert_eq!(projected["input"][2]["type"], "message");
    assert_eq!(
        projected["input"][2]["content"][0]["text"],
        "scheduled task fired"
    );
    assert_eq!(projected["input"][3]["type"], "reasoning");
    assert_eq!(
        projected["input"][3]["encrypted_content"],
        "opaque-encrypted-reasoning"
    );
    assert!(projected["input"][3].get("content").is_none());
    assert!(projected.get("tools").is_none());
    assert!(projected.get("tool_choice").is_none());
    assert!(projected.get("parallel_tool_calls").is_none());
}

#[test]
fn grok_rejects_encrypted_collaboration_history_before_transport() {
    let request = request(vec![ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/child".to_string(),
        content: vec![AgentMessageInputContent::EncryptedContent {
            encrypted_content: "opaque".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    }]);

    let error = ResponsesDialect::Grok
        .project_request(&request)
        .expect_err("encrypted collaboration history is not verified for Grok");
    assert!(
        error
            .to_string()
            .contains("encrypted collaboration history")
    );
}

#[test]
fn grok_rejects_unpaired_function_output_before_transport() {
    let request = request(vec![ResponseItem::FunctionCallOutput {
        id: None,
        call_id: None,
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text("orphan".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }]);

    let error = ResponsesDialect::Grok
        .project_request(&request)
        .expect_err("orphan function output must not reach Grok transport");
    assert!(error.to_string().contains("without call_id"));
}

#[test]
fn grok_projects_web_search_to_bare_hosted_contract_without_touching_flat_functions() {
    let mut canonical = request(vec![user_message("search")]);
    let tools = serde_json::value::to_raw_value(&json!([
        {
            "type": "function",
            "name": "local__apply_patch__deadbeefcafe",
            "description": "canonical `apply_patch` tool",
            "parameters": {"type":"object","properties":{"patch":{"type":"string"}},"required":["patch"],"additionalProperties":false},
            "strict": true
        },
        {
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true,
            "search_context_size": "medium"
        },
        {"type": "x_search"}
    ])).expect("tool JSON");
    canonical.tools = Some(ResponsesApiTools::from(std::sync::Arc::from(tools)));
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical)
        .expect("tool projection");
    assert_eq!(canonical, original, "canonical request must stay unchanged");
    assert_eq!(projected["tools"][0]["type"], "function");
    assert_eq!(
        projected["tools"][0]["name"],
        "local__apply_patch__deadbeefcafe"
    );
    assert_eq!(projected["tools"][1], json!({"type":"web_search"}));
    assert_eq!(projected["tools"][2], json!({"type":"x_search"}));
    assert_eq!(projected["tool_choice"], "auto");
    assert_eq!(projected["parallel_tool_calls"], true);
}

#[test]
fn stock_openai_projection_remains_identity() {
    let canonical = request(vec![user_message("stock")]);
    let expected = serde_json::to_value(&canonical).expect("stock request serializes");

    assert_eq!(
        ResponsesDialect::OpenAi
            .project_request(&canonical)
            .expect("stock projection"),
        expected
    );
    assert_eq!(expected["tool_choice"], json!("auto"));
    assert_eq!(expected["parallel_tool_calls"], json!(true));
}
