use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::remote_catalog_model;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::openai_models::ModelsResponse;
use core_test_support::responses;
use core_test_support::responses::mount_sse_once_match;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[tokio::test]
async fn thread_start_resolves_grok_model_to_provider_runtime() -> Result<()> {
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
    let grok_responses = mount_sse_once_match(
        &grok_server,
        header("authorization", "Bearer grok-test-key"),
        responses::sse(vec![
            responses::ev_response_created("grok-response"),
            responses::ev_assistant_message("grok-message", "Grok response"),
            responses::ev_completed("grok-response"),
        ]),
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

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .with_env_overrides(&[
            ("OPENAI_API_KEY", None),
            ("GROK_API_KEY", Some("grok-test-key")),
        ])
        .build_initialized()
        .await?;
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
