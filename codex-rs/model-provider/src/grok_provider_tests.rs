use codex_api::ResponsesDialect;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::ResponseItemId;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::PathBuf;

use crate::create_model_provider;
use crate::grok_catalog::static_model_catalog;

fn canonical_history(
    metadata: Option<InternalChatMessageMetadataPassthrough>,
    encrypted_function_args: Option<Vec<String>>,
) -> Vec<ResponseItem> {
    vec![
        ResponseItem::Message {
            id: None,
            role: "user".to_owned(),
            content: vec![ContentItem::InputText {
                text: "Use the weather tool.".to_owned(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: metadata.clone(),
        },
        ResponseItem::Message {
            id: None,
            role: "assistant".to_owned(),
            content: vec![ContentItem::OutputText {
                text: "Checking.".to_owned(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: metadata.clone(),
        },
        ResponseItem::FunctionCall {
            id: None,
            name: "weather".to_owned(),
            namespace: None,
            arguments: r#"{"city":"London"}"#.to_owned(),
            encrypted_function_args,
            call_id: "call-1".to_owned(),
            internal_chat_message_metadata_passthrough: metadata.clone(),
        },
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: Some("call-1".to_owned()),
            name: None,
            namespace: None,
            output: FunctionCallOutputPayload::from_text("rain".to_owned()),
            internal_chat_message_metadata_passthrough: metadata,
        },
    ]
}

fn canonical_agent_message(content: Vec<AgentMessageInputContent>) -> ResponseItem {
    ResponseItem::AgentMessage {
        id: Some(ResponseItemId::with_suffix("amsg", "child")),
        author: "/root".to_string(),
        recipient: "/root/child".to_string(),
        content,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn grok_projects_only_plaintext_agent_message_on_request_copy() {
    let grok = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );
    let envelope =
        "Message Type: NEW_TASK\nTask name: /root/child\nSender: /root\nPayload:\nreview";
    let input = vec![canonical_agent_message(vec![
        AgentMessageInputContent::InputText {
            text: envelope.to_string(),
        },
    ])];

    assert_eq!(
        grok.project_model_input(input),
        vec![ResponseItem::Message {
            id: Some(ResponseItemId::with_suffix("amsg", "child")),
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: envelope.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn grok_provider_projects_text_and_tool_continuation() {
    let provider = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );
    let input = canonical_history(
        Some(InternalChatMessageMetadataPassthrough {
            turn_id: Some("turn-1".to_owned()),
            ..Default::default()
        }),
        Some(vec!["encrypted".to_owned()]),
    );

    assert_eq!(
        provider.project_model_input(input),
        canonical_history(
            /*metadata*/ None, /*encrypted_function_args*/ None
        )
    );
}

#[test]
fn grok_provider_replays_encrypted_reasoning_without_null_content() {
    let provider = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );
    let input = vec![ResponseItem::Reasoning {
        id: Some(ResponseItemId::with_suffix("rs", "reasoning-id")),
        summary: vec![ReasoningItemReasoningSummary::SummaryText {
            text: "summary".to_owned(),
        }],
        content: None,
        encrypted_content: Some("opaque-encrypted-reasoning".to_owned()),
        internal_chat_message_metadata_passthrough: None,
    }];

    let projected = provider.project_model_input(input.clone());

    assert_eq!(
        serde_json::to_value(projected).expect("projected reasoning should serialize"),
        json!([{
            "id": "rs_reasoning-id",
            "type": "reasoning",
            "summary": [{
                "type": "summary_text",
                "text": "summary"
            }],
            "encrypted_content": "opaque-encrypted-reasoning"
        }])
    );
    assert_eq!(
        input,
        vec![ResponseItem::Reasoning {
            id: Some(ResponseItemId::with_suffix("rs", "reasoning-id")),
            summary: vec![ReasoningItemReasoningSummary::SummaryText {
                text: "summary".to_owned(),
            }],
            content: None,
            encrypted_content: Some("opaque-encrypted-reasoning".to_owned()),
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn grok_recognizes_only_completed_provider_hosted_x_calls() {
    let grok = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );
    let x_call = |name: &str, status: Option<&str>| ResponseItem::CustomToolCall {
        id: Some(ResponseItemId::with_suffix("ctc", "provider-item")),
        status: status.map(str::to_owned),
        call_id: "provider-call".to_owned(),
        name: name.to_owned(),
        namespace: None,
        input: "{}".to_owned(),
        internal_chat_message_metadata_passthrough: None,
    };

    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        assert!(grok.is_provider_hosted_tool_call(&x_call(name, Some("completed"))));
    }
    assert!(!grok.is_provider_hosted_tool_call(&x_call("x_keyword_search", None)));
    assert!(!grok.is_provider_hosted_tool_call(&x_call("x_keyword_search", Some("in_progress"))));
    assert!(!grok.is_provider_hosted_tool_call(&x_call("unverified_search", Some("completed"))));
}

#[tokio::test]
async fn resolved_provider_derives_internal_responses_dialect() {
    let grok = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );

    assert_eq!(
        grok.api_provider()
            .await
            .expect("Grok API provider")
            .responses_dialect,
        ResponsesDialect::Grok
    );
}

#[tokio::test]
async fn grok_models_manager_uses_bundle_or_exact_config_replacement() {
    let provider = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );
    let bundled_catalog = static_model_catalog();
    let bundled_models = provider
        .models_manager(PathBuf::new(), /*config_model_catalog*/ None)
        .get_remote_models()
        .await;

    assert_eq!(bundled_models, bundled_catalog.models);
    let mut replacement_model = static_model_catalog()
        .models
        .into_iter()
        .next()
        .expect("bundled Grok catalog should contain a model");
    replacement_model.slug = "configured-grok".to_string();
    let configured_catalog = ModelsResponse {
        models: vec![replacement_model],
    };

    assert_eq!(
        provider
            .models_manager(PathBuf::new(), Some(configured_catalog.clone()))
            .get_remote_models()
            .await,
        configured_catalog.models
    );
}
