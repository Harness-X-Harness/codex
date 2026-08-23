use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use crate::create_model_provider;

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
            call_id: "call-1".to_owned(),
            output: FunctionCallOutputPayload::from_text("rain".to_owned()),
            internal_chat_message_metadata_passthrough: metadata,
        },
    ]
}

#[test]
fn stock_provider_keeps_canonical_history_unchanged() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );
    let input = canonical_history(
        Some(InternalChatMessageMetadataPassthrough {
            turn_id: Some("turn-1".to_owned()),
            ..Default::default()
        }),
        Some(vec!["encrypted".to_owned()]),
    );

    assert_eq!(provider.project_model_input(input.clone()), input);
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
