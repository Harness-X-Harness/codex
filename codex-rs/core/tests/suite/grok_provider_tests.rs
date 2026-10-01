use codex_config::LoaderOverrides;
use codex_core::ModelClient;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_core::config::ConfigBuilder;
use codex_login::auth::AgentIdentityAuthPolicy;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;
use codex_rollout_trace::InferenceTraceContext;
use core_test_support::TestCodexResponsesRequestKind;
use core_test_support::responses::ev_completed;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses_metadata;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use wiremock::MockServer;

#[tokio::test]
async fn configured_grok_provider_projects_production_request() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let response = mount_sse_once(&server, sse(vec![ev_completed("r")])).await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "custom"
[model_providers.custom]
name = "Custom endpoint"
base_url = "{}/v1"
wire_api = "grok_responses"
"#,
            server.uri()
        ),
    )?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    assert_eq!(config.model_provider_id, "custom");
    let mut model = codex_core::test_support::construct_model_info_offline("gpt-5.4", &config);
    model.slug = "fixture-provider-model".into();
    model.use_responses_lite = false;
    model.supports_reasoning_summary_parameter = true;
    let thread = ThreadId::new();
    let telemetry = SessionTelemetry::new(
        thread,
        &model.slug,
        &model.slug,
        /*account_id*/ None,
        /*account_email*/ None,
        /*auth_mode*/ None,
        "test_originator".into(),
        /*log_user_prompts*/ false,
        "test".into(),
        SessionSource::Exec,
    );
    let client = ModelClient::new(
        /*auth_manager*/ None,
        AgentIdentityAuthPolicy::JwtOnly,
        thread,
        config.model_provider.clone(),
        SessionSource::Exec,
        "test_originator".into(),
        /*model_verbosity*/ None,
        /*content_item_kinds_enabled*/ false,
        /*reasoning_effort_override_enabled*/ false,
        /*enable_request_compression*/ false,
        /*include_timing_metrics*/ false,
        /*beta_features_header*/ None,
        /*concurrent_reasoning_summaries_enabled*/ false,
        /*attestation_provider*/ None,
        config.http_client_factory(),
        config.workspace_routing_context(),
        Vec::new(),
    );
    let metadata = responses_metadata(
        "11111111-1111-4111-8111-111111111111",
        "fixture-session",
        &thread.to_string(),
        /*turn_id*/ None,
        "fixture-window".into(),
        &SessionSource::Exec,
        /*parent_thread_id*/ None,
        TestCodexResponsesRequestKind::Turn,
    );
    let mut prompt = Prompt::default();
    prompt.input = serde_json::from_value::<Vec<ResponseItem>>(json!([
        {"type":"reasoning","id":"rs_saved","summary":[],"encrypted_content":"cipher",
            "content":[{"type":"reasoning_text","text":"private"}]},
        {"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}
    ]))?;
    prompt.base_instructions = BaseInstructions {
        text: "Say hi".into(),
        provenance: None,
    };
    let mut session = client.new_session();
    let mut stream = session
        .stream(
            &prompt,
            &model,
            &telemetry,
            Some(ReasoningEffort::High),
            ReasoningSummary::Detailed,
            /*service_tier*/ None,
            &metadata,
            &InferenceTraceContext::disabled(),
        )
        .await?;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        if let ResponseEvent::Completed { .. } = event? {
            completed = true;
        }
    }
    assert!(completed);
    assert_eq!(
        response.single_request().body_json(),
        json!({
            "model":"fixture-provider-model", "instructions":"Say hi",
            "input":[
                {"type":"reasoning","id":"rs_saved","summary":[],"encrypted_content":"cipher"},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}
            ],
            "reasoning":{"effort":"high","summary":"detailed"},
            "stream":true,"include":["reasoning.encrypted_content"],
            "prompt_cache_key":"fixture-session"
        })
    );
    Ok(())
}
