use codex_api::ResponsesDialect;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

use crate::create_model_provider;
use crate::grok_catalog::static_model_catalog;
use crate::provider::ProviderCapabilities;
use crate::provider::RemoteCompactionSupport;

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

#[test]
fn grok_advertises_only_proven_provider_capabilities() {
    let provider = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            namespace_tools: true,
            image_generation: false,
            web_search: false,
            external_web_access: false,
            remote_compaction: RemoteCompactionSupport::Unsupported,
        }
    );
}

#[test]
fn grok_does_not_inherit_stock_attestation() {
    let auth_manager = AuthManager::from_auth_for_testing(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
    );
    let stock = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(auth_manager.clone()),
    );
    let grok = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        Some(auth_manager),
    );

    assert!(stock.supports_attestation());
    assert!(!grok.supports_attestation());
}

#[tokio::test]
async fn resolved_provider_derives_internal_responses_dialect() {
    let stock = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );
    let grok = create_model_provider(
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
    );

    assert_eq!(
        stock
            .api_provider()
            .await
            .expect("stock API provider")
            .responses_dialect,
        ResponsesDialect::OpenAi
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

    assert_eq!(
        provider
            .models_manager(PathBuf::new(), /*config_model_catalog*/ None)
            .get_remote_models()
            .await,
        bundled_catalog.models
    );

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
