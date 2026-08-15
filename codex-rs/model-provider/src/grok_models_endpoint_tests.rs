use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::model_info::BASE_INSTRUCTIONS;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::*;
use crate::provider::create_model_provider;

fn grok_provider_info(base_url: String) -> ModelProviderInfo {
    let mut provider = ModelProviderInfo::create_openai_provider(Some(base_url));
    provider.name = "Grok".to_string();
    provider.wire_api = WireApi::GrokResponses;
    provider.provider_adapter = Some(codex_model_provider_info::ModelProviderAdapter::Grok);
    provider.requires_openai_auth = false;
    provider
}

fn expected_model(slug: &str) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: String::new(),
        description: None,
        default_reasoning_level: None,
        supported_reasoning_levels: Vec::new(),
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority: 99,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        availability_nux: None,
        upgrade: None,
        model_messages: Some(ModelMessages {
            instructions_template: Some(BASE_INSTRUCTIONS.to_string()),
            instructions_variables: None,
            approvals: None,
            collaboration_modes: None,
            auto_review: None,
            permissions: None,
            token_budget: None,
        }),
        include_skills_usage_instructions: false,
        include_plugin_usage_instructions: false,
        include_apps_usage_instructions: false,
        supports_reasoning_summary_parameter: true,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: WebSearchToolType::Text,
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_parallel_tool_calls: false,
        supports_image_detail_original: false,
        context_window: None,
        max_context_window: None,
        auto_compact_token_limit: None,
        comp_hash: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: Vec::new(),
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        api_backend: None,
        supports_backend_search: false,
        use_responses_lite: false,
        auto_review_model_override: None,
        model_specialty: None,
        tool_mode: None,
        multi_agent_version: None,
    }
}

#[tokio::test]
async fn public_models_manager_preserves_authoritative_grok_catalog() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{
                "id": "catalog-id",
                "model": "grok-4.6",
                "name": "Grok 4.6",
                "description": "Provider description",
                "context_window": 500_000,
                "api_backend": "responses",
                "supports_reasoning_effort": true,
                "reasoning_effort": "high",
                "reasoning_efforts": [
                    {"value": "xhigh", "description": "Deep", "default": true},
                    {"value": "high", "description": "High", "default": true},
                    {"value": "medium", "description": "Medium"},
                    {"value": "low", "description": "Low"}
                ],
                "auto_compact_threshold_percent": 80,
                "supports_backend_search": true,
                "unknown_additive_field": {"future": true}
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let provider =
        create_model_provider(grok_provider_info(server.uri()), /*auth_manager*/ None);
    let manager = provider.models_manager_without_cache(Some(ModelsResponse {
        models: vec![expected_model("operator-invented")],
    }));

    let catalog = manager
        .raw_model_catalog(
            RefreshStrategy::Online,
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await;
    let mut expected = expected_model("grok-4.6");
    expected.display_name = "Grok 4.6".to_string();
    expected.description = Some("Provider description".to_string());
    expected.default_reasoning_level = Some(ReasoningEffort::High);
    expected.supported_reasoning_levels = vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::XHigh,
            description: "Deep".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "High".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Medium".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Low".to_string(),
        },
    ];
    expected.context_window = Some(500_000);
    expected.max_context_window = Some(500_000);
    expected.auto_compact_token_limit = Some(400_000);
    expected.api_backend = Some("responses".to_string());
    expected.supports_backend_search = true;

    assert_eq!(catalog.models, vec![expected]);
}

#[test]
fn id_only_entry_invents_no_provider_capability() {
    let model = decode_model(&json!({"id": "future-grok"})).expect("identity is valid");

    assert_eq!(model, expected_model("future-grok"));
}

#[test]
fn invalid_identity_is_isolated_from_valid_entries() {
    let entries = json!([
        {"id": ""},
        {"name": "missing identity"},
        {"id": "valid", "future": [1, 2, 3]}
    ]);
    let models = entries
        .as_array()
        .expect("entries are an array")
        .iter()
        .filter_map(|value| decode_model(value).ok())
        .collect::<Vec<_>>();

    assert_eq!(models, vec![expected_model("valid")]);
}

#[test]
fn effort_gate_preserves_unknown_values_and_uses_single_option_default() {
    let model = decode_model(&json!({
        "id": "future-grok",
        "supportsReasoningEffort": true,
        "reasoningEfforts": [
            {"value": "quantum", "label": "Quantum", "default": true},
            "low"
        ]
    }))
    .expect("model is valid");
    let mut expected = expected_model("future-grok");
    expected.default_reasoning_level = Some(ReasoningEffort::Custom("quantum".to_string()));
    expected.supported_reasoning_levels = vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Custom("quantum".to_string()),
            description: "Quantum".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "low".to_string(),
        },
    ];

    assert_eq!(model, expected);
}

#[test]
fn disabled_effort_gate_ignores_stray_effort_fields() {
    let model = decode_model(&json!({
        "id": "plain-grok",
        "supports_reasoning_effort": false,
        "reasoning_effort": "high",
        "reasoning_efforts": ["high"]
    }))
    .expect("model is valid");

    assert_eq!(model, expected_model("plain-grok"));
}

#[test]
fn incompatible_backend_is_not_selectable() {
    let model = decode_model(&json!({
        "id": "chat-only-grok",
        "apiBackend": "chat_completions"
    }))
    .expect("model is valid");
    let mut expected = expected_model("chat-only-grok");
    expected.api_backend = Some("chat_completions".to_string());
    expected.supported_in_api = false;
    expected.visibility = ModelVisibility::Hide;

    assert_eq!(model, expected);
}

#[tokio::test]
async fn grok_endpoint_rejects_codex_catalog_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
        .expect(1)
        .mount(&server)
        .await;
    let endpoint =
        GrokModelsEndpoint::new(grok_provider_info(server.uri()), /*auth_manager*/ None);

    let error = endpoint
        .list_models(
            "0.0.0",
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await
        .expect_err("Grok must not infer a different catalog envelope");

    assert!(
        error
            .to_string()
            .contains("failed to decode models response")
    );
}

#[tokio::test]
async fn provider_name_and_wire_dialect_do_not_select_the_grok_catalog_strategy() {
    let server = MockServer::start().await;
    let expected = expected_model("ordinary-model");
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
            models: vec![expected.clone()],
        }))
        .expect(1)
        .mount(&server)
        .await;
    let mut provider_info = ModelProviderInfo::create_openai_provider(Some(server.uri()));
    provider_info.name = "Grok".to_string();
    provider_info.wire_api = WireApi::GrokResponses;
    provider_info.provider_adapter =
        Some(codex_model_provider_info::ModelProviderAdapter::Configured);
    provider_info.requires_openai_auth = false;
    provider_info.experimental_bearer_token = Some("configured-test-token".to_string());
    let provider = create_model_provider(provider_info, /*auth_manager*/ None);
    let manager = provider.models_manager_without_cache(/*config_model_catalog*/ None);

    let catalog = manager
        .raw_model_catalog(
            RefreshStrategy::Online,
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await;

    assert_eq!(catalog.models, vec![expected]);
}
