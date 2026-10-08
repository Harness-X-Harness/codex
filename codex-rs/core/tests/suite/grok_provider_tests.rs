use codex_config::LoaderOverrides;
use codex_core::CodexResponsesMetadata;
use codex_core::ModelClient;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_core::config::ConfigBuilder;
use codex_login::auth::AgentIdentityAuthPolicy;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
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

#[path = "grok_compaction_tests.rs"]
mod compaction;

#[path = "grok_hosted_search_tests.rs"]
mod hosted_search;

#[path = "grok_tool_roundtrip_tests.rs"]
mod tool_roundtrip;

struct ProviderFixture {
    _home: TempDir,
    server: MockServer,
    model: ModelInfo,
    client: ModelClient,
    telemetry: SessionTelemetry,
    metadata: CodexResponsesMetadata,
    prompt: Prompt,
}

async fn provider_fixture(provider_name: &str) -> anyhow::Result<ProviderFixture> {
    let server = MockServer::start().await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "custom"
[model_providers.custom]
name = "{provider_name}"
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
    // The infallible public runtime also accepts metadata that bypasses config validation.
    let mut provider = config.model_provider.clone();
    provider.supports_websockets = true;
    let client = ModelClient::new(
        /*auth_manager*/ None,
        AgentIdentityAuthPolicy::JwtOnly,
        thread,
        provider,
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
    Ok(ProviderFixture {
        _home: home,
        server,
        model,
        client,
        telemetry,
        metadata,
        prompt,
    })
}

#[tokio::test]
async fn configured_grok_provider_projects_production_request_and_stream() -> anyhow::Result<()> {
    let message = json!({"type":"message","id":"msg_live","role":"assistant","content":[]});
    let reasoning = json!({"type":"reasoning","id":"rs_live","summary":[],"content":null,"encrypted_content":null});
    for (frames, expected) in [
        (
            vec![
                json!({"type":"response.output_item.added","output_index":1,"item":reasoning}),
                json!({"type":"response.reasoning_summary_text.delta","output_index":1,"summary_index":0,"delta":"R"}),
                json!({"type":"response.output_item.added","output_index":0,"item":message}),
                json!({"type":"response.output_text.delta","output_index":0,"delta":"A"}),
                json!({"type":"response.output_item.done","output_index":1,"item":reasoning}),
                json!({"type":"response.output_item.done","output_index":0,"item":message}),
                ev_completed("r"),
            ],
            json!([
                {"added":message},{"text":"A"},{"done":message},
                {"added":reasoning},{"summary":"R","index":0},{"done":reasoning},
                {"completed":"r"}
            ]),
        ),
        (
            vec![
                json!({"type":"response.output_item.added","output_index":0,"item":message}),
                ev_completed("r"),
                ev_completed("must-not-complete"),
            ],
            json!([
                {"added":message},
                {"error":"Grok stream: completion has an open item or missing output index"}
            ]),
        ),
    ] {
        let fixture = provider_fixture("Custom endpoint").await?;
        let response = mount_sse_once(&fixture.server, sse(frames)).await;
        let mut session = fixture.client.new_session();
        session
            .preconnect_websocket(
                &fixture.model,
                /*service_tier*/ None,
                &fixture.telemetry,
                &fixture.metadata,
            )
            .await?;
        session
            .prewarm_websocket(
                &fixture.prompt,
                &fixture.model,
                &fixture.telemetry,
                Some(ReasoningEffort::High),
                ReasoningSummary::Detailed,
                /*service_tier*/ None,
                &fixture.metadata,
            )
            .await?;
        assert!(
            fixture
                .server
                .received_requests()
                .await
                .expect("request recording")
                .is_empty()
        );
        let mut stream = session
            .stream(
                &fixture.prompt,
                &fixture.model,
                &fixture.telemetry,
                Some(ReasoningEffort::High),
                ReasoningSummary::Detailed,
                /*service_tier*/ None,
                &fixture.metadata,
                &InferenceTraceContext::disabled(),
            )
            .await?;
        let mut trace = Vec::new();
        while let Some(event) = stream.next().await {
            trace.push(match event {
                Ok(ResponseEvent::OutputItemAdded(item)) => json!({"added":item}),
                Ok(ResponseEvent::OutputItemDone(item)) => json!({"done":item}),
                Ok(ResponseEvent::OutputTextDelta(text)) => json!({"text":text}),
                Ok(ResponseEvent::ReasoningSummaryDelta {
                    delta,
                    summary_index,
                }) => json!({"summary":delta,"index":summary_index}),
                Ok(ResponseEvent::Completed { response_id, .. }) => {
                    json!({"completed":response_id})
                }
                Ok(ResponseEvent::RateLimits(_)) => continue,
                Err(error) => {
                    let CodexErrorDetails::Stream(message) = error.details() else {
                        return Err(error.into());
                    };
                    json!({"error":message})
                }
                other => anyhow::bail!("unexpected event: {other:?}"),
            });
        }
        assert_eq!(json!(trace), expected);
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
    }
    Ok(())
}

#[tokio::test]
async fn configured_grok_rejects_unsupported_history_before_transport() -> anyhow::Result<()> {
    let mut fixture = provider_fixture("Custom endpoint").await?;
    fixture.prompt.input = vec![ResponseItem::Other];
    let mut session = fixture.client.new_session();
    let Err(error) = session
        .stream(
            &fixture.prompt,
            &fixture.model,
            &fixture.telemetry,
            Some(ReasoningEffort::High),
            ReasoningSummary::Detailed,
            /*service_tier*/ None,
            &fixture.metadata,
            &InferenceTraceContext::disabled(),
        )
        .await
    else {
        anyhow::bail!("unsupported Grok history must fail before transport");
    };
    let CodexErrorDetails::Stream(message) = error.details() else {
        return Err(error.into());
    };
    assert_eq!(
        message,
        "Grok Basic/reasoning cannot replay this history at input[0]"
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn grok_collaboration_plaintext_replay_ignores_provider_display_name() -> anyhow::Result<()> {
    for provider_name in ["Grok", "OpenAI", "Azure"] {
        let mut fixture = provider_fixture(provider_name).await?;
        let canonical: Vec<ResponseItem> = serde_json::from_value(json!([
            {"type":"function_call", "id":"fc_collaboration", "call_id":"collaboration-call",
                "namespace":"collaboration", "name":"send_message", "arguments":"{ \"message\": \"private\" }",
                "encrypted_function_args":[]},
            {"type":"function_call_output", "call_id":"collaboration-call", "output":"delivered"}
        ]))?;
        fixture.prompt.input = canonical.clone();
        let response = mount_sse_once(&fixture.server, sse(vec![ev_completed("replayed")])).await;
        let mut session = fixture.client.new_session();
        let mut stream = session
            .stream(
                &fixture.prompt,
                &fixture.model,
                &fixture.telemetry,
                Some(ReasoningEffort::High),
                ReasoningSummary::Detailed,
                /*service_tier*/ None,
                &fixture.metadata,
                &InferenceTraceContext::disabled(),
            )
            .await?;
        let mut completed = false;
        while let Some(event) = stream.next().await {
            if matches!(event?, ResponseEvent::Completed { .. }) {
                completed = true;
            }
        }
        assert!(completed);
        assert_eq!(
            fixture.prompt.input, canonical,
            "canonical logging marker must remain"
        );
        let wire_name = codex_tools::flat_wire_name(
            "function",
            &codex_tools::ToolName::namespaced("collaboration", "send_message"),
        );
        assert_eq!(
            response.single_request().body_json()["input"],
            json!([
                {"type":"function_call", "id":"fc_collaboration", "call_id":"collaboration-call",
                    "name":wire_name, "arguments":"{ \"message\": \"private\" }"},
                {"type":"function_call_output", "call_id":"collaboration-call", "output":"delivered"}
            ])
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_unsupported_tool_plans_still_fail_before_transport() -> anyhow::Result<()> {
    use codex_core::StartThreadOptions;
    use codex_core::TurnInputRequest;
    use codex_protocol::config_types::WebSearchConfig;
    use codex_protocol::config_types::WebSearchMode;
    use codex_protocol::config_types::WebSearchUserLocation;
    use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
    use codex_protocol::dynamic_tools::DynamicToolSpec;
    use codex_protocol::openai_models::ConfigShellToolType;
    use codex_protocol::openai_models::ToolMode;
    use codex_protocol::openai_models::WebSearchToolType;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::user_input::UserInput;
    use core_test_support::test_codex::test_codex;
    use core_test_support::wait_for_event;
    use std::sync::Arc;

    for (web_search_mode, unsupported) in [
        (WebSearchMode::Live, "web_search restrictions"),
        (WebSearchMode::Disabled, "tool_search"),
    ] {
        let server = MockServer::start().await;
        let home = Arc::new(TempDir::new()?);
        let base_url = format!("{}/v1", server.uri());
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                r#"
model_provider = "grok"
approval_policy = "never"
sandbox_mode = "read-only"
[model_providers.grok]
name = "Grok"
base_url = "{base_url}"
wire_api = "grok_responses"
requires_openai_auth = false
supports_websockets = false
[tools.update_plan]
enabled = false
[tools.experimental_request_user_input]
enabled = false
[features]
goals = false
shell_tool = false
view_image = false
sleep_tool = false
multi_agent = false
multi_agent_v2 = false
code_mode = false
apps = false
image_generation = false
tool_suggest = false
standalone_web_search = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
"#
            ),
        )?;
        let test = test_codex()
            .with_home(home)
            .with_model_info_override("gpt-5.4", |model| {
                model.shell_type = ConfigShellToolType::Disabled;
                model.apply_patch_tool_type = None;
                model.structured_edit_tool_type = None;
                model.tool_mode = Some(ToolMode::Direct);
                model.supports_search_tool = true;
                model.use_responses_lite = false;
                model.web_search_tool_type = WebSearchToolType::Text;
            })
            .with_config(move |config| {
                config.model_provider = config.model_providers["grok"].clone();
                config.model_provider.stream_max_retries = Some(0);
                if web_search_mode == WebSearchMode::Live {
                    config.web_search_config = Some(WebSearchConfig {
                        user_location: Some(WebSearchUserLocation {
                            country: Some("US".into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                }
                config
                    .web_search_mode
                    .set(web_search_mode)
                    .expect("test web search mode should be accepted");
            })
            .build_with_auto_env(&server)
            .await?;
        let mut dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
            name: "local_tool".into(),
            description: "An eager local function.".into(),
            input_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
            defer_loading: false,
        })];
        if unsupported == "tool_search" {
            dynamic_tools.push(DynamicToolSpec::Function(DynamicToolFunctionSpec {
                name: "deferred_lookup".into(),
                description: "A discoverable deferred function.".into(),
                input_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
                defer_loading: true,
            }));
        }
        // Exercise production plan construction and request projection via public APIs.
        let codex_core::NewThread { thread, .. } = test
            .thread_manager
            .start_thread(StartThreadOptions {
                dynamic_tools,
                ..StartThreadOptions::new(test.config.clone())
            })
            .await?;
        thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Use the available tools.".into(),
                text_elements: Vec::new(),
            }]))
            .await?;
        let EventMsg::TurnComplete(completed) =
            wait_for_event(&thread, |event| matches!(event, EventMsg::TurnComplete(_))).await
        else {
            unreachable!("event predicate guarantees turn completion");
        };
        let error = completed
            .error
            .expect("unsupported tool plan must fail the turn");
        let expected_error = if unsupported == "tool_search" {
            "flat local tools do not support tool_search"
        } else {
            "Grok cannot project web_search restrictions"
        };
        assert!(
            error.message.contains(expected_error),
            "unexpected error: {error:?}"
        );
        assert!(
            server
                .received_requests()
                .await
                .expect("request recording")
                .is_empty(),
            "keeping hosted/search in the canonical plan must never silently omit it on the wire"
        );
    }
    Ok(())
}
