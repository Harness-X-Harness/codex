use super::build;
use crate::common::Reasoning;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::common::TextControls;
use crate::common::TextFormat;
use crate::common::TextFormatType;
use crate::provider::Provider;
use crate::provider::ResponsesDialect;
use crate::provider::RetryConfig;
use crate::provider::XSearchProviderConfig;
use codex_protocol::ResponseItemId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ConfigurationReasoning;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::LocalShellAction;
use codex_protocol::models::LocalShellExecAction;
use codex_protocol::models::LocalShellStatus;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::openai_models::ReasoningEffort;
use http::HeaderMap;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
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
        x_search: None,
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

fn json_tools(value: Value) -> ResponsesApiTools {
    let tools = serde_json::value::to_raw_value(&value).expect("tool JSON");
    ResponsesApiTools::from(std::sync::Arc::from(tools))
}

fn encrypted_reasoning(
    encrypted_content: Option<&str>,
    content: Option<Vec<ReasoningItemContent>>,
) -> ResponseItem {
    ResponseItem::Reasoning {
        id: Some(ResponseItemId::with_suffix("rs", "reasoning-id")),
        summary: vec![ReasoningItemReasoningSummary::SummaryText {
            text: "summary".to_string(),
        }],
        content,
        encrypted_content: encrypted_content.map(str::to_string),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(key) || object.values().any(|child| contains_key(child, key))
        }
        Value::Array(items) => items.iter().any(|child| contains_key(child, key)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn accepted_fixtures() -> Vec<(&'static str, ResponsesApiRequest)> {
    let envelope =
        "Message Type: NEW_TASK\nTask name: /root/child\nSender: /root\nPayload:\nreview";
    let mut function_custom_web_search = request(vec![user_message("search")]);
    function_custom_web_search.tools = Some(json_tools(json!([
        {
            "type": "function",
            "name": "local__apply_patch__deadbeefcafe",
            "description": "canonical `apply_patch` tool",
            "parameters": {"type":"object","properties":{"patch":{"type":"string"}},"required":["patch"],"additionalProperties":false},
            "strict": true
        },
        {
            "type": "custom",
            "name": "apply_patch",
            "description": "freeform patch",
            "format": {"type":"grammar","syntax":"lark","definition":"start: \"a\""}
        },
        {
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true,
            "search_context_size": "medium"
        }
    ])));

    let mut nested_search_extras = request(vec![user_message("search")]);
    nested_search_extras.tools = Some(json_tools(json!([
        {
            "type": "function",
            "name": "local__wait__deadbeefcafe",
            "defer_loading": true,
            "parameters": {"type":"object","properties":{}}
        },
        {
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true
        }
    ])));
    nested_search_extras.client_metadata = Some(HashMap::from([(
        "note".to_string(),
        "external_web_access".to_string(),
    )]));

    let mut kitchen_sink = request(vec![user_message("kitchen")]);
    kitchen_sink.parallel_tool_calls = false;
    kitchen_sink.store = false;
    kitchen_sink.prompt_cache_key = Some("cache-key".to_string());
    kitchen_sink.client_metadata = Some(HashMap::from([("app".to_string(), "codex".to_string())]));
    kitchen_sink.reasoning = Some(Reasoning {
        effort: Some(ReasoningEffort::XHigh),
        summary: Some(ReasoningSummary::Auto),
        context: None,
    });
    kitchen_sink.text = Some(TextControls {
        verbosity: None,
        format: Some(TextFormat {
            r#type: TextFormatType::JsonSchema,
            strict: true,
            schema: json!({"type":"object","properties":{}}),
            name: "codex_output_schema".to_string(),
        }),
    });
    kitchen_sink.tools = Some(json_tools(json!([{
        "type": "function",
        "name": "stock_function",
        "description": "stock",
        "parameters": {"type":"object","properties":{},"additionalProperties":false},
        "strict": true
    }])));

    vec![
        ("no_tools", request(vec![user_message("hello")])),
        (
            "function_custom_web_search_tools",
            function_custom_web_search,
        ),
        ("web_search_allowed_domains", {
            let mut req = request(vec![user_message("search")]);
            req.tools = Some(json_tools(json!([{
                "type": "web_search",
                "external_web_access": true,
                "indexed_web_access": true,
                "search_context_size": "medium",
                "user_location": {"type": "approximate", "country": "US"},
                "search_content_types": ["text"],
                "filters": {
                    "allowed_domains": ["example.com"],
                    "excluded_domains": ["blocked.test"]
                }
            }])));
            req
        }),
        (
            "message_input_text",
            request(vec![user_message("input_text")]),
        ),
        (
            "message_input_image",
            request(vec![ResponseItem::Message {
                id: Some(ResponseItemId::with_suffix("msg", "image")),
                role: "user".to_string(),
                content: vec![ContentItem::InputImage {
                    image_url: "https://example.test/a.png".to_string(),
                    detail: Some(ImageDetail::High),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "message_output_text",
            request(vec![ResponseItem::Message {
                id: Some(ResponseItemId::with_suffix("msg", "assistant")),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "done".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "agent_message_plaintext",
            request(vec![ResponseItem::AgentMessage {
                id: Some(ResponseItemId::with_suffix("amsg", "child")),
                author: "/root".to_string(),
                recipient: "/root/child".to_string(),
                content: vec![AgentMessageInputContent::InputText {
                    text: envelope.to_string(),
                }],
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "reasoning_with_blob",
            request(vec![encrypted_reasoning(
                Some("opaque-encrypted-reasoning"),
                Some(vec![ReasoningItemContent::ReasoningText {
                    text: "first-turn trace".to_string(),
                }]),
            )]),
        ),
        (
            "reasoning_without_blob",
            request(vec![encrypted_reasoning(
                /*encrypted_content*/ None, /*content*/ None,
            )]),
        ),
        (
            "reasoning_with_content_no_blob",
            request(vec![encrypted_reasoning(
                /*encrypted_content*/ None,
                Some(vec![ReasoningItemContent::ReasoningText {
                    text: "trace".to_string(),
                }]),
            )]),
        ),
        (
            "reasoning_empty_encrypted_content",
            request(vec![encrypted_reasoning(
                Some(""),
                Some(vec![ReasoningItemContent::ReasoningText {
                    text: "wiped".to_string(),
                }]),
            )]),
        ),
        (
            "function_call",
            request(vec![ResponseItem::FunctionCall {
                id: Some(ResponseItemId::with_suffix("fc", "1")),
                name: "local__exec_command__deadbeef".to_string(),
                namespace: Some("local".to_string()),
                arguments: "{}".to_string(),
                encrypted_function_args: Some(vec!["enc".to_string()]),
                call_id: "call_1".to_string(),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "function_call_output_text",
            request(vec![ResponseItem::FunctionCallOutput {
                id: Some(ResponseItemId::with_suffix("fco", "1")),
                call_id: Some("call_1".to_string()),
                name: Some("local__exec_command__deadbeef".to_string()),
                namespace: Some("local".to_string()),
                output: FunctionCallOutputPayload::from_text("ok".to_string()),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "function_call_output_content_items",
            request(vec![ResponseItem::FunctionCallOutput {
                id: None,
                call_id: Some("call_items".to_string()),
                name: None,
                namespace: None,
                output: FunctionCallOutputPayload::from_content_items(vec![
                    FunctionCallOutputContentItem::InputText {
                        text: "item-out".to_string(),
                    },
                ]),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "named_unpaired_function_call_output",
            request(vec![ResponseItem::FunctionCallOutput {
                id: None,
                call_id: None,
                name: Some("notify".to_string()),
                namespace: None,
                output: FunctionCallOutputPayload::from_text("scheduled task fired".to_string()),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "custom_tool_call",
            request(vec![ResponseItem::CustomToolCall {
                id: Some(ResponseItemId::with_suffix("ctc", "x")),
                status: Some("completed".to_string()),
                call_id: "call_2".to_string(),
                name: "x_keyword_search".to_string(),
                namespace: Some("x".to_string()),
                input: "{}".to_string(),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "custom_tool_call_output",
            request(vec![ResponseItem::CustomToolCallOutput {
                id: Some(ResponseItemId::with_suffix("ctco", "1")),
                call_id: "call_patch".to_string(),
                name: Some("apply_patch".to_string()),
                output: FunctionCallOutputPayload::from_text("patched".to_string()),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "web_search_call",
            request(vec![ResponseItem::WebSearchCall {
                id: Some(ResponseItemId::with_suffix("ws", "1")),
                status: Some("completed".to_string()),
                action: Some(WebSearchAction::Search {
                    query: Some("weather".to_string()),
                    queries: None,
                }),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "image_generation_call",
            request(vec![ResponseItem::ImageGenerationCall {
                id: Some(ResponseItemId::with_suffix("ig", "1")),
                status: "completed".to_string(),
                revised_prompt: Some("a cat".to_string()),
                result: "data".to_string(),
                internal_chat_message_metadata_passthrough: None,
            }]),
        ),
        (
            "compaction_trigger_dropped",
            request(vec![
                ResponseItem::CompactionTrigger {},
                user_message("continue"),
            ]),
        ),
        (
            "replayed_history",
            request(vec![
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
                    output: FunctionCallOutputPayload::from_text(
                        "scheduled task fired".to_string(),
                    ),
                    internal_chat_message_metadata_passthrough: None,
                },
                encrypted_reasoning(Some("opaque-encrypted-reasoning"), /*content*/ None),
            ]),
        ),
        (
            "openai_only_history_controls",
            request(vec![
                ResponseItem::FunctionCall {
                    id: None,
                    name: "local__exec_command__deadbeef".to_string(),
                    namespace: Some("local".to_string()),
                    arguments: "{}".to_string(),
                    encrypted_function_args: Some(vec!["enc".to_string()]),
                    call_id: "call_1".to_string(),
                    internal_chat_message_metadata_passthrough: None,
                },
                ResponseItem::CustomToolCall {
                    id: None,
                    status: Some("completed".to_string()),
                    call_id: "call_2".to_string(),
                    name: "x_keyword_search".to_string(),
                    namespace: Some("x".to_string()),
                    input: "{}".to_string(),
                    internal_chat_message_metadata_passthrough: None,
                },
                ResponseItem::CompactionTrigger {},
                user_message("continue"),
            ]),
        ),
        ("nested_search_extras", nested_search_extras),
        (
            "reasoning_text_format_prompt_cache_client_metadata_store_parallel",
            kitchen_sink,
        ),
        ("x_search_already_present", {
            let mut req = request(vec![user_message("x")]);
            req.tools = Some(json_tools(json!([
                {"type": "function", "name": "f", "description": "d", "parameters": {"type":"object"}, "strict": false},
                {"type": "x_search"}
            ])));
            req
        }),
    ]
}

#[test]
fn grok_builds_every_accepted_fixture_without_openai_only_keys() {
    for (name, req) in accepted_fixtures() {
        let built = build(&req, &provider("Grok")).unwrap_or_else(|err| panic!("{name}: {err}"));
        let mut leaked = Vec::new();
        for key in [
            "stream_options",
            "service_tier",
            "access_programs",
            "parallel_tool_calls",
            "store",
            "client_metadata",
        ] {
            if built.get(key).is_some() {
                leaked.push(key.to_string());
            }
        }
        if built["reasoning"].get("context").is_some() {
            leaked.push("reasoning.context".to_string());
        }
        if built["text"].get("verbosity").is_some() {
            leaked.push("text.verbosity".to_string());
        }
        for (index, item) in built["input"].as_array().into_iter().flatten().enumerate() {
            for key in ["namespace", "encrypted_function_args", "phase", "status"] {
                if item.get(key).is_some() {
                    leaked.push(format!("input[{index}].{key}"));
                }
            }
        }
        for (index, tool) in built["tools"].as_array().into_iter().flatten().enumerate() {
            for key in [
                "external_web_access",
                "indexed_web_access",
                "user_location",
                "search_context_size",
                "search_content_types",
                "defer_loading",
                "strict",
            ] {
                if tool.get(key).is_some() {
                    leaked.push(format!("tools[{index}].{key}"));
                }
            }
            if tool
                .get("filters")
                .and_then(|filters| filters.get("excluded_domains"))
                .is_some()
            {
                leaked.push(format!("tools[{index}].filters.excluded_domains"));
            }
            if tool.get("type").and_then(Value::as_str) == Some("x_search")
                && let Some(object) = tool.as_object()
            {
                for key in object.keys() {
                    if !matches!(key.as_str(), "type" | "from_date" | "to_date") {
                        leaked.push(format!("tools[{index}].{key}"));
                    }
                }
            }
        }
        assert_eq!(leaked, Vec::<String>::new(), "{name}: {built}");
    }
}

#[test]
fn grok_drops_other_input_items() {
    let req = request(vec![ResponseItem::Other, user_message("after-other")]);
    let built = build(&req, &provider("Grok")).expect("Other is dropped, not rejected");
    let input = built["input"].as_array().expect("input array");
    assert_eq!(input.len(), 1, "Other must not be replayed: {built}");
    assert_eq!(input[0]["type"], "message");
}

#[test]
fn responses_dialect_is_selected_for_grok_identity_or_host() {
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

    let mut xai = provider("xAI");
    xai.base_url = "https://api.x.ai/v1".to_string();
    assert_eq!(ResponsesDialect::for_provider(&xai), ResponsesDialect::Grok);

    let mut tunnel = provider("Custom");
    tunnel.base_url = "https://grok.trustedtunnel.app/v1".to_string();
    assert_eq!(
        ResponsesDialect::for_provider(&tunnel),
        ResponsesDialect::Grok
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
        .project_request(&canonical, &provider("Grok"))
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
    assert_eq!(projected.get("parallel_tool_calls"), None);
    assert_eq!(projected.get("store"), None);
    assert_eq!(projected.get("client_metadata"), None);
}

#[test]
fn grok_omits_reasoning_content_when_replaying_encrypted_blob() {
    let canonical = request(vec![
        user_message("你好"),
        encrypted_reasoning(
            Some("opaque-encrypted-reasoning"),
            Some(vec![ReasoningItemContent::ReasoningText {
                text: "first-turn trace".to_string(),
            }]),
        ),
    ]);
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("Grok should replay encrypted reasoning without a content channel");

    assert_eq!(
        canonical, original,
        "durable/canonical request must not mutate"
    );
    assert_eq!(projected["input"][1]["type"], "reasoning");
    assert_eq!(
        projected["input"][1]["encrypted_content"],
        "opaque-encrypted-reasoning"
    );
    assert!(
        projected["input"][1].get("content").is_none(),
        "Grok treats a content channel as a modified compaction blob: {}",
        projected["input"][1]
    );
}

#[test]
fn grok_omits_null_encrypted_reasoning_blob() {
    let canonical = request(vec![encrypted_reasoning(
        /*encrypted_content*/ None, /*content*/ None,
    )]);
    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("Grok should omit a null reasoning blob");

    assert_eq!(projected["input"][0]["type"], "reasoning");
    assert!(
        projected["input"][0].get("encrypted_content").is_none(),
        "null encrypted_content is also reported as a compaction blob: {}",
        projected["input"][0]
    );
    assert!(projected["input"][0].get("content").is_none());
}

#[test]
fn grok_strips_openai_only_history_controls_from_replay() {
    let canonical = request(vec![
        ResponseItem::FunctionCall {
            id: None,
            name: "local__exec_command__deadbeef".to_string(),
            namespace: Some("local".to_string()),
            arguments: "{}".to_string(),
            encrypted_function_args: Some(vec!["enc".to_string()]),
            call_id: "call_1".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CustomToolCall {
            id: None,
            status: Some("completed".to_string()),
            call_id: "call_2".to_string(),
            name: "x_keyword_search".to_string(),
            namespace: Some("x".to_string()),
            input: "{}".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CompactionTrigger {},
        user_message("continue"),
    ]);
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("OpenAI-only history extras should project");

    assert_eq!(canonical, original, "canonical request must stay unchanged");
    let input = projected["input"].as_array().expect("input array");
    assert_eq!(
        input.len(),
        3,
        "compaction_trigger is not a Grok input item"
    );
    assert_eq!(input[0]["type"], "function_call");
    assert_eq!(input[0]["name"], "local__exec_command__deadbeef");
    assert_eq!(input[0]["call_id"], "call_1");
    assert!(input[0].get("namespace").is_none());
    assert!(input[0].get("encrypted_function_args").is_none());
    assert_eq!(input[1]["type"], "custom_tool_call");
    assert_eq!(input[1]["name"], "x_keyword_search");
    assert!(input[1].get("namespace").is_none());
    assert!(input[1].get("status").is_none());
    assert_eq!(input[2]["type"], "message");
}

#[test]
fn grok_rejects_encrypted_collaboration_history_before_transport() {
    let request = request(vec![
        user_message("keep"),
        ResponseItem::AgentMessage {
            id: None,
            author: "/root".to_string(),
            recipient: "/root/child".to_string(),
            content: vec![AgentMessageInputContent::EncryptedContent {
                encrypted_content: "opaque".to_string(),
            }],
            internal_chat_message_metadata_passthrough: None,
        },
    ]);

    let error = ResponsesDialect::Grok
        .project_request(&request, &provider("Grok"))
        .expect_err("encrypted collaboration history is not verified for Grok");
    let message = error.to_string();
    assert!(message.contains("encrypted collaboration history"));
    assert!(message.contains("input[1]"), "{message}");
}

#[test]
fn grok_rejects_unpaired_function_output_before_transport() {
    let request = request(vec![
        user_message("keep"),
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: None,
            namespace: None,
            output: FunctionCallOutputPayload::from_text("orphan".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
    ]);

    let error = ResponsesDialect::Grok
        .project_request(&request, &provider("Grok"))
        .expect_err("orphan function output must not reach Grok transport");
    let message = error.to_string();
    assert!(message.contains("without call_id"));
    assert!(message.contains("input[1]"), "{message}");
}

#[test]
fn grok_projects_web_and_x_search_contract_without_touching_flat_functions() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.tools = Some(json_tools(json!([
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
        }
    ])));
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("tool projection");
    assert_eq!(canonical, original, "canonical request must stay unchanged");
    assert_eq!(
        projected["tools"],
        json!([
            {
                "type": "function",
                "name": "local__apply_patch__deadbeefcafe",
                "description": "canonical `apply_patch` tool",
                "parameters": {
                    "type": "object",
                    "properties": {"patch": {"type": "string"}},
                    "required": ["patch"],
                    "additionalProperties": false
                }
            },
            {"type": "web_search"},
            {"type": "x_search"}
        ])
    );
    assert_eq!(projected["tool_choice"], "auto");
    assert_eq!(projected.get("parallel_tool_calls"), None);
    assert_eq!(projected.get("store"), None);
    assert_eq!(projected.get("client_metadata"), None);
    assert!(
        !contains_key(&projected, "external_web_access"),
        "Grok egress must not send external_web_access"
    );
}

#[test]
fn grok_projects_web_search_allowed_domains_from_stock_filters() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.tools = Some(json_tools(json!([
        {
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true,
            "search_context_size": "medium",
            "search_content_types": ["text"],
            "user_location": {
                "type": "approximate",
                "country": "US"
            },
            "filters": {
                "allowed_domains": ["example.com"],
                "excluded_domains": ["blocked.test"]
            }
        }
    ])));
    let original = canonical.clone();

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("web_search allowed_domains should project");

    assert_eq!(canonical, original, "canonical request must stay unchanged");
    assert_eq!(
        projected["tools"],
        json!([
            {
                "type": "web_search",
                "filters": {"allowed_domains": ["example.com"]}
            },
            {"type": "x_search"}
        ])
    );
    assert!(
        !contains_key(&projected, "external_web_access"),
        "Grok egress must not send external_web_access"
    );
    assert!(
        !contains_key(&projected, "excluded_domains"),
        "Grok egress must not send excluded_domains"
    );
}

#[test]
fn grok_projects_bare_web_search_when_filters_are_missing_or_empty() {
    for (name, web_search) in [
        ("missing", json!({"type": "web_search"})),
        ("empty_object", json!({"type": "web_search", "filters": {}})),
        (
            "empty_list",
            json!({"type": "web_search", "filters": {"allowed_domains": []}}),
        ),
    ] {
        let mut canonical = request(vec![user_message("search")]);
        canonical.tools = Some(json_tools(json!([web_search])));
        let projected = ResponsesDialect::Grok
            .project_request(&canonical, &provider("Grok"))
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(
            projected["tools"],
            json!([{"type": "web_search"}, {"type": "x_search"}]),
            "{name}"
        );
    }
}

#[test]
fn grok_caps_web_search_allowed_domains_at_five() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.tools = Some(json_tools(json!([{
        "type": "web_search",
        "filters": {
            "allowed_domains": [
                "a.example",
                "b.example",
                "c.example",
                "d.example",
                "e.example",
                "f.example"
            ]
        }
    }])));

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("capped allowed_domains should project");

    assert_eq!(
        projected["tools"],
        json!([
            {
                "type": "web_search",
                "filters": {
                    "allowed_domains": [
                        "a.example",
                        "b.example",
                        "c.example",
                        "d.example",
                        "e.example"
                    ]
                }
            },
            {"type": "x_search"}
        ])
    );
}

#[test]
fn grok_strips_external_web_access_from_nested_request_payloads() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.instructions = "external_web_access".to_string();
    canonical.tools = Some(json_tools(json!([
        {
            "type": "function",
            "name": "local__wait__deadbeefcafe",
            "defer_loading": true,
            "parameters": {"type":"object","properties":{}}
        },
        {
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true
        }
    ])));
    canonical.client_metadata = Some(HashMap::from([(
        "note".to_string(),
        "external_web_access".to_string(),
    )]));

    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("nested search extras should project");

    assert_eq!(
        projected["tools"],
        json!([
            {
                "type": "function",
                "name": "local__wait__deadbeefcafe",
                "parameters": {"type": "object", "properties": {}}
            },
            {"type": "web_search"},
            {"type": "x_search"}
        ])
    );
    assert_eq!(
        projected["instructions"], "external_web_access",
        "string fields must keep the phrase; only JSON arguments are stripped"
    );
    assert_eq!(projected.get("client_metadata"), None);
    assert!(
        !contains_key(&projected, "external_web_access"),
        "nested OpenAI search arguments must not reach Grok"
    );
    assert!(
        !contains_key(&projected, "indexed_web_access"),
        "nested OpenAI search arguments must not reach Grok"
    );
    assert!(
        !contains_key(&projected, "defer_loading"),
        "OpenAI deferred-tool extras must not reach Grok"
    );
}

#[test]
fn stock_openai_keeps_reasoning_content_with_encrypted_blob() {
    let canonical = request(vec![encrypted_reasoning(
        Some("openai-blob"),
        Some(vec![ReasoningItemContent::ReasoningText {
            text: "stock trace".to_string(),
        }]),
    )]);
    let expected = serde_json::to_value(&canonical).expect("stock request serializes");

    assert_eq!(
        ResponsesDialect::OpenAi
            .project_request(&canonical, &provider("OpenAI"))
            .expect("stock projection"),
        expected
    );
    assert_eq!(expected["input"][0]["encrypted_content"], "openai-blob");
    assert_eq!(
        expected["input"][0]["content"][0]["text"], "stock trace",
        "OpenAI binds the blob to the original content channel"
    );
}

#[test]
fn stock_openai_projection_remains_identity() {
    let mut canonical = request(vec![user_message("stock")]);
    canonical.tools = Some(json_tools(json!([{
        "type": "function",
        "name": "stock_function",
        "description": "stock",
        "parameters": {"type":"object","properties":{},"additionalProperties":false},
        "strict": true
    }])));
    canonical.client_metadata = Some(HashMap::from([("app".to_string(), "codex".to_string())]));
    let expected = serde_json::to_value(&canonical).expect("stock request serializes");

    assert_eq!(
        ResponsesDialect::OpenAi
            .project_request(&canonical, &provider("OpenAI"))
            .expect("stock projection"),
        expected
    );
    assert_eq!(expected["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(expected["tool_choice"], json!("auto"));
    assert_eq!(expected["parallel_tool_calls"], json!(true));
    assert_eq!(expected["store"], json!(false));
    assert_eq!(expected["client_metadata"], json!({"app": "codex"}));
}

fn local_shell_call() -> ResponseItem {
    ResponseItem::LocalShellCall {
        id: None,
        call_id: None,
        status: LocalShellStatus::Completed,
        action: LocalShellAction::Exec(LocalShellExecAction {
            command: vec!["echo".to_string()],
            timeout_ms: None,
            working_directory: None,
            env: None,
            user: None,
        }),
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn grok_rejects_unsupported_input_items_before_transport() {
    let cases: [(&str, ResponseItem); 7] = [
        (
            "configuration_update",
            ResponseItem::ConfigurationUpdate {
                reasoning: ConfigurationReasoning {
                    effort: ReasoningEffort::Medium,
                },
            },
        ),
        (
            "compaction",
            ResponseItem::Compaction {
                id: None,
                encrypted_content: "blob".to_string(),
                internal_chat_message_metadata_passthrough: None,
            },
        ),
        (
            "context_compaction",
            ResponseItem::ContextCompaction {
                id: None,
                encrypted_content: None,
                internal_chat_message_metadata_passthrough: None,
            },
        ),
        ("local_shell_call", local_shell_call()),
        (
            "tool_search_call",
            ResponseItem::ToolSearchCall {
                id: None,
                call_id: None,
                status: None,
                execution: "client".to_string(),
                arguments: json!({}),
                internal_chat_message_metadata_passthrough: None,
            },
        ),
        (
            "tool_search_output",
            ResponseItem::ToolSearchOutput {
                id: None,
                call_id: None,
                status: "completed".to_string(),
                execution: "client".to_string(),
                tools: Vec::new(),
                internal_chat_message_metadata_passthrough: None,
            },
        ),
        (
            "additional_tools",
            ResponseItem::AdditionalTools {
                id: None,
                role: "user".to_string(),
                tools: Vec::new(),
            },
        ),
    ];

    for (variant, item) in cases {
        let req = request(vec![user_message("keep"), item]);
        let error = build(&req, &provider("Grok")).expect_err(variant);
        let message = error.to_string();
        assert!(
            message.contains(variant),
            "{variant} should be named in {message}"
        );
        assert!(
            message.contains("input[1]"),
            "{variant} should name the index in {message}"
        );
    }
}

#[test]
fn grok_rejects_unsupported_tools_before_transport() {
    let cases = [
        (
            0,
            "namespace",
            json!([{"type":"namespace","name":"local","description":"","tools":[]}]),
        ),
        (
            0,
            "tool_search",
            json!([{
                "type":"tool_search",
                "execution":"client",
                "description":"search",
                "parameters":{"type":"object"}
            }]),
        ),
        (0, "code_interpreter", json!([{"type":"code_interpreter"}])),
        (
            1,
            "namespace",
            json!([
                {
                    "type":"function",
                    "name":"f",
                    "description":"d",
                    "parameters":{"type":"object"},
                    "strict":true
                },
                {"type":"namespace","name":"local","description":"","tools":[]}
            ]),
        ),
    ];

    for (index, tool_type, tools) in cases {
        let mut req = request(vec![user_message("keep")]);
        req.tools = Some(json_tools(tools));
        let error = build(&req, &provider("Grok")).expect_err(tool_type);
        let message = error.to_string();
        assert!(
            message.contains(tool_type),
            "{tool_type} should be named in {message}"
        );
        let needle = format!("tools[{index}]");
        assert!(
            message.contains(&needle),
            "{tool_type} should name {needle} in {message}"
        );
    }
}

fn grok_x_search_window(from_date: &str, to_date: &str) -> Provider {
    let mut grok = provider("Grok");
    grok.x_search = Some(XSearchProviderConfig {
        from_date: Some(from_date.to_string()),
        to_date: Some(to_date.to_string()),
    });
    grok
}

#[test]
fn grok_appends_bare_x_search_without_date_keys() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.tools = Some(json_tools(json!([{"type": "web_search"}])));
    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("bare x_search append");
    assert_eq!(
        projected["tools"],
        json!([{"type": "web_search"}, {"type": "x_search"}])
    );
}

#[test]
fn grok_appends_x_search_date_window_from_provider() {
    let mut canonical = request(vec![user_message("search")]);
    canonical.tools = Some(json_tools(json!([{"type": "web_search"}])));
    let projected = ResponsesDialect::Grok
        .project_request(
            &canonical,
            &grok_x_search_window("2026-01-01", "2026-01-31"),
        )
        .expect("provider x_search window should project");
    assert_eq!(
        projected["tools"],
        json!([
            {"type": "web_search"},
            {
                "type": "x_search",
                "from_date": "2026-01-01",
                "to_date": "2026-01-31"
            }
        ])
    );
}

#[test]
fn grok_copies_canonical_x_search_dates_over_provider_window() {
    let mut canonical = request(vec![user_message("x")]);
    canonical.tools = Some(json_tools(json!([{
        "type": "x_search",
        "from_date": "2026-02-01",
        "to_date": "2026-02-28"
    }])));
    let projected = ResponsesDialect::Grok
        .project_request(
            &canonical,
            &grok_x_search_window("2026-01-01", "2026-01-31"),
        )
        .expect("canonical x_search dates should win");
    assert_eq!(
        projected["tools"],
        json!([{
            "type": "x_search",
            "from_date": "2026-02-01",
            "to_date": "2026-02-28"
        }])
    );
}

#[test]
fn grok_fills_provider_window_when_canonical_x_search_has_no_dates() {
    let mut canonical = request(vec![user_message("x")]);
    canonical.tools = Some(json_tools(json!([{"type": "x_search"}])));
    let projected = ResponsesDialect::Grok
        .project_request(
            &canonical,
            &grok_x_search_window("2026-01-01", "2026-01-31"),
        )
        .expect("provider window should fill a bare canonical x_search");
    assert_eq!(
        projected["tools"],
        json!([{
            "type": "x_search",
            "from_date": "2026-01-01",
            "to_date": "2026-01-31"
        }])
    );
}

#[test]
fn grok_omits_invalid_x_search_dates_from_tool_json() {
    let mut canonical = request(vec![user_message("x")]);
    canonical.tools = Some(json_tools(json!([{
        "type": "x_search",
        "from_date": "not-a-date",
        "to_date": "2026-13-40"
    }])));
    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("invalid tool JSON dates should be omitted");
    assert_eq!(projected["tools"], json!([{"type": "x_search"}]));
}

#[test]
fn grok_omits_unexpected_keys_on_x_search() {
    let mut canonical = request(vec![user_message("x")]);
    canonical.tools = Some(json_tools(json!([{
        "type": "x_search",
        "from_date": "2026-01-01",
        "enabled": true,
        "filters": {"allowed_domains": ["x.com"]}
    }])));
    let projected = ResponsesDialect::Grok
        .project_request(&canonical, &provider("Grok"))
        .expect("unexpected x_search keys should be dropped");
    assert_eq!(
        projected["tools"],
        json!([{
            "type": "x_search",
            "from_date": "2026-01-01"
        }])
    );
}
