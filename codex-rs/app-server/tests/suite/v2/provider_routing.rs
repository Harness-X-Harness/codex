use anyhow::Context;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::remote_catalog_model;
use app_test_support::to_response;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::openai_models::ModelsResponse;
use core_test_support::responses;
use core_test_support::responses::mount_sse_once_match;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

struct ProviderRoutingFixture {
    codex_home: TempDir,
    openai_server: MockServer,
    grok_server: MockServer,
}

impl ProviderRoutingFixture {
    async fn new() -> Result<Self> {
        let openai_server = MockServer::start().await;
        let grok_server = MockServer::start().await;
        mount_models_repeating(
            &openai_server,
            ModelsResponse {
                models: vec![remote_catalog_model("openai-model", "ChatGPT Model")],
            },
        )
        .await;
        mount_models_repeating(
            &grok_server,
            ModelsResponse {
                models: vec![remote_catalog_model("grok-model", "Grok Model")],
            },
        )
        .await;

        let codex_home = TempDir::new()?;
        std::fs::write(
            codex_home.path().join("config.toml"),
            format!(
                r#"
model = "openai-model"
model_provider = "openai"
approval_policy = "never"
sandbox_mode = "read-only"
web_search = "live"
openai_base_url = "{}/v1"

[model_providers.grok]
name = "Grok"
base_url = "{}/v1"
env_key = "GROK_API_KEY"
wire_api = "grok_responses"
"#,
                openai_server.uri(),
                grok_server.uri(),
            ),
        )?;
        write_chatgpt_auth(
            codex_home.path(),
            ChatGptAuthFixture::new("chatgpt-access-token").plan_type("pro"),
            AuthCredentialsStoreMode::File,
        )?;

        Ok(Self {
            codex_home,
            openai_server,
            grok_server,
        })
    }

    async fn start_app(&self) -> Result<TestAppServer> {
        TestAppServer::builder()
            .with_codex_home(self.codex_home.path())
            .with_env_overrides(&[
                ("OPENAI_API_KEY", None),
                ("GROK_API_KEY", Some("grok-test-key")),
            ])
            .build_initialized()
            .await
    }
}

#[tokio::test]
async fn thread_start_resolves_grok_model_to_provider_runtime() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = mount_sse_once_match(
        &fixture.grok_server,
        header("authorization", "Bearer grok-test-key"),
        responses::sse(vec![
            responses::ev_response_created("grok-response"),
            responses::ev_assistant_message("grok-message", "Grok response"),
            responses::ev_completed("grok-response"),
        ]),
    )
    .await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    assert_eq!(started.model, "grok-model");
    assert_eq!(started.model_provider, "grok");

    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: started.thread.id,
        input: vec![UserInput::Text {
            text: "Use the selected provider".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    })
    .await?;

    assert_eq!(grok_responses.requests().len(), 1);
    assert_eq!(
        grok_responses.single_request().body_json()["model"],
        "grok-model"
    );
    Ok(())
}

#[tokio::test]
async fn turn_rejects_a_model_owned_by_another_provider_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;

    let request_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: started.thread.id,
            input: vec![UserInput::Text {
                text: "Keep this thread on its provider".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("new thread"));
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn fork_inherits_the_source_thread_provider_binding() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.grok_server, "grok-fork-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &started.thread.id).await?;

    let request_id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: started.thread.id,
            ..Default::default()
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadForkResponse { thread, .. } = to_response(response)?;

    assert_eq!(thread.model_provider, "grok");
    Ok(())
}

#[tokio::test]
async fn cold_resume_rejects_a_conflicting_provider_binding() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.grok_server, "grok-resume-seed").await;
    let mut primary = fixture.start_app().await?;
    let started = primary
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut primary, &started.thread.id).await?;
    timeout(DEFAULT_TIMEOUT, primary.shutdown_gracefully()).await??;

    let mut secondary = fixture.start_app().await?;
    let request_id = secondary
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id,
            model: Some("openai-model".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        secondary.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("new thread"));
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn thread_list_defaults_to_all_configured_provider_profiles() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.openai_server, "openai-list-seed").await;
    mount_completion(&fixture.grok_server, "grok-list-seed").await;
    let mut app = fixture.start_app().await?;
    let openai_thread = app.start_thread(ThreadStartParams::default()).await?;
    materialize_thread(&mut app, &openai_thread.thread.id).await?;
    let grok_thread = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &grok_thread.thread.id).await?;

    let request_id = app
        .send_thread_list_request(ThreadListParams {
            cursor: None,
            limit: Some(20),
            sort_key: None,
            sort_direction: None,
            model_providers: None,
            source_kinds: None,
            archived: None,
            section_id: None,
            cwd: None,
            use_state_db_only: false,
            search_term: None,
            parent_thread_id: None,
            ancestor_thread_id: None,
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadListResponse { data, .. } = to_response(response)?;

    assert!(
        data.iter()
            .any(|thread| thread.id == openai_thread.thread.id)
    );
    assert!(data.iter().any(|thread| thread.id == grok_thread.thread.id));
    Ok(())
}

async fn mount_models_repeating(server: &MockServer, body: ModelsResponse) {
    Mock::given(method("GET"))
        .and(path_regex(".*/models$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(body),
        )
        .mount(server)
        .await;
}

async fn received_responses_count(server: &MockServer) -> Result<usize> {
    let requests = server
        .received_requests()
        .await
        .context("wiremock did not record requests")?;
    Ok(requests
        .iter()
        .filter(|request| {
            request.method.as_str() == "POST" && request.url.path().ends_with("/responses")
        })
        .count())
}

async fn mount_completion(server: &MockServer, response_id: &str) {
    mount_sse_once_match(
        server,
        method("POST"),
        responses::sse(vec![
            responses::ev_response_created(response_id),
            responses::ev_assistant_message(&format!("{response_id}-message"), "Seed response"),
            responses::ev_completed(response_id),
        ]),
    )
    .await;
}

async fn materialize_thread(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread_id.to_string(),
        input: vec![UserInput::Text {
            text: "Persist this thread".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    })
    .await?;
    Ok(())
}
