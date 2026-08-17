use anyhow::Context;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::remote_catalog_model;
use app_test_support::to_response;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ImageGenerationItem;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ReviewDelivery;
use codex_app_server_protocol::ReviewStartParams;
use codex_app_server_protocol::ReviewStartResponse;
use codex_app_server_protocol::ReviewTarget;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartedNotification;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_config::types::AuthCredentialsStoreMode;
use codex_core::RolloutRecorder;
use codex_model_provider::provider_models_home;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::RolloutItem;
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
const TINY_PNG_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const TINY_PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

mod runtime;

struct ProviderRoutingFixture {
    codex_home: TempDir,
    openai_server: MockServer,
    grok_server: MockServer,
}

impl ProviderRoutingFixture {
    async fn new() -> Result<Self> {
        Self::with_review_model(/*review_model*/ None).await
    }

    async fn with_review_model(review_model: Option<&str>) -> Result<Self> {
        Self::with_config(
            "model = \"openai-model\"\nmodel_provider = \"openai\"\n",
            review_model,
            /*multi_agent_v2*/ false,
            /*register_grok*/ true,
        )
        .await
    }

    async fn with_implicit_openai_default() -> Result<Self> {
        Self::with_config(
            "", /*review_model*/ None, /*multi_agent_v2*/ false,
            /*register_grok*/ true,
        )
        .await
    }

    async fn with_implicit_openai_default_and_multi_agent_v2() -> Result<Self> {
        Self::with_config(
            "", /*review_model*/ None, /*multi_agent_v2*/ true,
            /*register_grok*/ true,
        )
        .await
    }

    async fn with_stock_openai_only() -> Result<Self> {
        Self::with_config(
            "model = \"unlisted-openai-model\"\nmodel_provider = \"openai\"\n",
            /*review_model*/ None,
            /*multi_agent_v2*/ false,
            /*register_grok*/ false,
        )
        .await
    }

    async fn with_config(
        default_selection: &str,
        review_model: Option<&str>,
        multi_agent_v2: bool,
        register_grok: bool,
    ) -> Result<Self> {
        let openai_server = MockServer::start().await;
        let grok_server = MockServer::start().await;
        mount_models_repeating(
            &openai_server,
            ModelsResponse {
                models: vec![
                    remote_catalog_model("openai-model", "ChatGPT Model"),
                    remote_catalog_model("openai-model-2", "ChatGPT Model 2"),
                ],
            },
        )
        .await;
        mount_grok_models_repeating(&grok_server, &["grok-model"]).await;

        let codex_home = TempDir::new()?;
        let review_model = review_model
            .map(|model| format!("review_model = \"{model}\"\n"))
            .unwrap_or_default();
        let multi_agent_v2 = format!("multi_agent_v2 = {multi_agent_v2}");
        let registrations =
            register_grok.then_some("model_provider_registrations = [\"openai\", \"grok\"]\n");
        std::fs::write(
            codex_home.path().join("config.toml"),
            format!(
                r#"
{default_selection}{review_model}approval_policy = "never"
sandbox_mode = "read-only"
web_search = "live"
openai_base_url = "{}/v1"
{registrations}

[model_providers.grok]
name = "Grok"
base_url = "{}/v1"
env_key = "GROK_API_KEY"
provider_adapter = "grok"
wire_api = "grok_responses"
x_search = true

[features]
enable_request_compression = false
{multi_agent_v2}
"#,
                openai_server.uri(),
                grok_server.uri(),
                registrations = registrations.unwrap_or_default(),
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

    fn disable_grok_x_search(&self) -> Result<()> {
        self.replace_config_line("x_search = true", "x_search = false")
    }

    fn enable_code_mode(&self) -> Result<()> {
        self.replace_config_line(
            "[features]\nenable_request_compression = false",
            "[features]\ncode_mode = true\nenable_request_compression = false",
        )
    }

    fn disable_image_generation(&self) -> Result<()> {
        self.replace_config_line(
            "[features]\nenable_request_compression = false",
            "[features]\nimage_generation = false\nenable_request_compression = false",
        )
    }

    fn enable_auto_compaction(&self, token_limit: i64) -> Result<()> {
        self.replace_config_line(
            "approval_policy = \"never\"",
            &format!(
                "approval_policy = \"never\"\ncompact_prompt = \"Summarize the conversation.\"\nmodel_auto_compact_token_limit = {token_limit}"
            ),
        )
    }

    fn replace_config_line(&self, current: &str, replacement: &str) -> Result<()> {
        let path = self.codex_home.path().join("config.toml");
        let config = std::fs::read_to_string(&path)?;
        anyhow::ensure!(
            config.contains(current),
            "test config does not contain the expected Grok gate"
        );
        std::fs::write(path, config.replacen(current, replacement, 1))?;
        Ok(())
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
async fn grok_model_without_backend_search_omits_web_and_x_tools() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    fixture.grok_server.reset().await;
    mount_grok_models_with_backend_search(&fixture.grok_server, &["grok-model"], false).await;
    let response = mount_completion(&fixture.grok_server, "grok-no-search").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    app.start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;

    let body = response.single_request().body_json();
    let tool_types = body["tools"]
        .as_array()
        .context("Grok request tools should be an array")?
        .iter()
        .filter_map(|tool| tool.get("type").and_then(serde_json::Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!tool_types.contains("web_search"));
    assert!(!tool_types.contains("x_search"));
    assert!(tool_types.contains("image_generation"));
    Ok(())
}

#[tokio::test]
async fn turn_fails_before_egress_when_bound_provider_authority_is_unavailable() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let seed_response = mount_completion(&fixture.grok_server, "grok-authority-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &started.thread.id).await?;
    assert_eq!(seed_response.requests().len(), 1);

    fixture.grok_server.reset().await;
    Mock::given(method("GET"))
        .and(path_regex(".*/models$"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&fixture.grok_server)
        .await;
    std::fs::remove_dir_all(provider_models_home(fixture.codex_home.path(), "grok"))?;

    let request_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: started.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Do not egress without an authoritative model".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = timeout(DEFAULT_TIMEOUT, app.read_response(request_id)).await??;
    let error: ErrorNotification =
        timeout(DEFAULT_TIMEOUT, app.read_notification("error")).await??;

    assert!(error.error.message.contains("AuthorityUnavailable"));
    assert!(!error.will_retry);
    assert_eq!(error.thread_id, started.thread.id);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn grok_app_turn_declares_and_persists_native_hosted_tools() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-web-response"),
                responses::ev_web_search_call_added_partial("web-1", "in_progress"),
                responses::ev_web_search_call_added_partial("web-1", "in_progress"),
                responses::ev_web_search_call_done("web-1", "completed", "xAI"),
                responses::ev_web_search_call_done("web-1", "completed", "xAI"),
                responses::ev_assistant_message("grok-web-message", "Done"),
                responses::ev_completed("grok-web-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-x-response"),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "x-1",
                        "type": "custom_tool_call",
                        "status": "in_progress",
                        "call_id": "x-call-1",
                        "name": "x_keyword_search",
                        "input": ""
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "x-1",
                        "type": "custom_tool_call",
                        "status": "in_progress",
                        "call_id": "x-call-1",
                        "name": "x_keyword_search",
                        "input": ""
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "x-1",
                        "type": "custom_tool_call",
                        "status": "completed",
                        "call_id": "x-call-1",
                        "name": "x_keyword_search",
                        "input": "{\"query\":\"xAI\"}"
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "x-1",
                        "type": "custom_tool_call",
                        "status": "completed",
                        "call_id": "x-call-1",
                        "name": "x_keyword_search",
                        "input": "{\"query\":\"xAI\"}"
                    }
                }),
                responses::ev_assistant_message("grok-x-message", "Done"),
                responses::ev_completed("grok-x-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-image-response"),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "image-1",
                        "type": "image_generation_call",
                        "status": "in_progress",
                        "result": null
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "image-1",
                        "type": "image_generation_call",
                        "status": "in_progress",
                        "result": null
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "image-1",
                        "type": "image_generation_call",
                        "status": "completed",
                        "prompt": "Draw a blue circle.",
                        "result": TINY_PNG_DATA_URL
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "image-1",
                        "type": "image_generation_call",
                        "status": "completed",
                        "prompt": "Draw a blue circle.",
                        "result": TINY_PNG_DATA_URL
                    }
                }),
                responses::ev_assistant_message("grok-image-message", "Done"),
                responses::ev_completed("grok-image-response"),
            ]),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    let mut thread_ids = Vec::new();
    let mut live_image_path = None;
    for prompt in [
        "Exercise Web Search",
        "Exercise X Search",
        "Exercise Image Generation",
    ] {
        let started = app
            .start_thread(ThreadStartParams {
                model: Some("grok-model".to_string()),
                ..Default::default()
            })
            .await?;
        let turn_params = TurnStartParams {
            thread_id: started.thread.id.clone(),
            input: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        };
        if prompt == "Exercise Image Generation" {
            let request_id = app.send_turn_start_request(turn_params).await?;
            let _: TurnStartResponse =
                timeout(DEFAULT_TIMEOUT, app.read_response(request_id)).await??;
            loop {
                let completed: ItemCompletedNotification =
                    timeout(DEFAULT_TIMEOUT, app.read_notification("item/completed")).await??;
                let ThreadItem::ImageGeneration(ImageGenerationItem {
                    saved_path: Some(saved_path),
                    ..
                }) = completed.item
                else {
                    continue;
                };
                assert_eq!(std::fs::read(&saved_path)?, TINY_PNG_BYTES);
                live_image_path = Some(saved_path);
                break;
            }
            timeout(
                DEFAULT_TIMEOUT,
                app.read_stream_until_notification_message("turn/completed"),
            )
            .await??;
        } else {
            app.start_turn_and_wait_for_completion(turn_params).await?;
        }
        thread_ids.push(started.thread.id);
    }

    let requests = grok_responses.requests();
    assert_eq!(requests.len(), 3);
    for request in requests {
        let body = request.body_json();
        let tool_types = body["tools"]
            .as_array()
            .context("Grok request tools should be an array")?
            .iter()
            .filter_map(|tool| tool.get("type").and_then(serde_json::Value::as_str))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(body["tool_choice"], "auto");
        assert!(tool_types.contains("web_search"));
        assert!(tool_types.contains("x_search"));
        assert!(tool_types.contains("image_generation"));
    }

    let expected = [
        ("webSearch", "web-1", None),
        ("webSearch", "x-1", Some("x")),
        ("imageGeneration", "image-1", None),
    ];
    for (thread_id, (item_type, item_id, source)) in thread_ids.into_iter().zip(expected) {
        let read_id = app
            .send_thread_read_request(ThreadReadParams {
                thread_id,
                include_turns: true,
            })
            .await?;
        let read_response = timeout(
            DEFAULT_TIMEOUT,
            app.read_stream_until_response_message(RequestId::Integer(read_id)),
        )
        .await??;
        let ThreadReadResponse { thread, .. } = to_response(read_response)?;
        let persisted_items = thread
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            persisted_items
                .iter()
                .filter(|item| {
                    item["type"] == item_type
                        && item["id"] == item_id
                        && source.is_none_or(|source| item["source"] == source)
                })
                .count(),
            1,
            "hosted item must be projected and persisted exactly once"
        );
        if item_type == "imageGeneration" {
            let image = persisted_items
                .iter()
                .find(|item| item["type"] == item_type && item["id"] == item_id)
                .context("persisted image item should exist")?;
            assert_eq!(
                image["savedPath"],
                serde_json::to_value(live_image_path.as_ref())?,
                "thread/read must preserve the readable image artifact path"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn grok_app_request_preserves_complete_freeform_grammar_metadata() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    fixture.enable_code_mode()?;
    let response = mount_sse_once_match(
        &fixture.grok_server,
        header("authorization", "Bearer grok-test-key"),
        responses::sse(vec![
            responses::ev_assistant_message("grok-freeform-message", "Done"),
            responses::ev_completed("grok-freeform-response"),
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

    let completed = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    let body = response.single_request().body_json();
    let exec_description = body["tools"]
        .as_array()
        .context("Grok request tools should be an array")?
        .iter()
        .find(|tool| tool["type"] == "function" && tool["name"] == "exec")
        .and_then(|tool| tool["description"].as_str())
        .context("Grok request should contain the projected code-mode exec tool")?;
    let (_, serialized_format) = exec_description
        .rsplit_once("Local Codex freeform grammar metadata (descriptive only; the Grok Gateway does not enforce this grammar):\n")
        .context("exec description should contain the descriptive grammar boundary")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(serialized_format)?,
        serde_json::json!({
            "type": "grammar",
            "syntax": "lark",
            "definition": "\nstart: pragma_source | plain_source\npragma_source: PRAGMA_LINE NEWLINE SOURCE\nplain_source: SOURCE\n\nPRAGMA_LINE: /[ \\t]*\\/\\/ @exec:[^\\r\\n]*/\nNEWLINE: /\\r?\\n/\nSOURCE: /[\\s\\S]+/\n"
        })
    );
    Ok(())
}

#[tokio::test]
async fn grok_undeclared_hosted_output_fails_before_projection_or_local_dispatch() -> Result<()> {
    {
        let fixture = ProviderRoutingFixture::new().await?;
        fixture.grok_server.reset().await;
        mount_grok_models_with_backend_search(&fixture.grok_server, &["grok-model"], false).await;
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("undeclared-web-response"),
                responses::ev_web_search_call_done("web-undeclared", "completed", "not-owned"),
                responses::ev_completed("undeclared-web-response"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("was not declared"))
        );
        assert_eq!(
            response.requests().len(),
            1,
            "must not retry or dispatch locally"
        );
        assert!(!request_declares_tool(
            &response.single_request().body_json(),
            "web_search"
        ));
        assert_rollout_omits_hosted_item(&rollout_path, "web-undeclared").await?;
    }

    {
        let fixture = ProviderRoutingFixture::new().await?;
        fixture.disable_grok_x_search()?;
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("undeclared-x-response"),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "x-undeclared",
                        "type": "custom_tool_call",
                        "status": "in_progress",
                        "call_id": "x-call-undeclared",
                        "name": "x_keyword_search",
                        "input": ""
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "x-undeclared",
                        "type": "custom_tool_call",
                        "status": "completed",
                        "call_id": "x-call-undeclared",
                        "name": "x_keyword_search",
                        "input": "{\"query\":\"preserve me\"}"
                    }
                }),
                responses::ev_completed("undeclared-x-response"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("was not declared"))
        );
        assert_eq!(
            response.requests().len(),
            1,
            "must not retry or dispatch locally"
        );
        assert!(!request_declares_tool(
            &response.single_request().body_json(),
            "x_search"
        ));
        assert_rollout_preserves_unknown_custom_item(
            &rollout_path,
            Some("x-undeclared"),
            "x-call-undeclared",
        )
        .await?;
    }

    {
        let fixture = ProviderRoutingFixture::new().await?;
        let arguments = serde_json::json!({
            "plan": [{
                "step": "must not dispatch after ownership changes",
                "status": "completed"
            }]
        })
        .to_string();
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("ordinary-to-hosted-owner-change"),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "shared-owner-reverse",
                        "type": "function_call",
                        "call_id": "queued-must-not-dispatch",
                        "name": "update_plan",
                        "arguments": arguments
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "shared-owner-reverse",
                        "type": "custom_tool_call",
                        "status": "in_progress",
                        "call_id": "x-owner-change-call",
                        "name": "x_keyword_search",
                        "input": ""
                    }
                }),
                responses::ev_completed("ordinary-to-hosted-owner-change"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("changed Tool Plan ownership"))
        );
        assert_eq!(response.requests().len(), 1, "must not retry");
        assert_rollout_tool_output_count(
            &rollout_path,
            "queued-must-not-dispatch",
            /*expected*/ 0,
        )
        .await?;
        assert_eq!(
            app.pending_notification_methods()
                .iter()
                .filter(|method| method.as_str() == "turn/plan/updated")
                .count(),
            0,
            "a rejected Grok response must not dispatch an already queued local tool"
        );
    }

    {
        let fixture = ProviderRoutingFixture::new().await?;
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("malformed-x-response"),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "status": "completed",
                        "call_id": "x-call-missing-id",
                        "name": "x_keyword_search",
                        "input": "{\"query\":\"preserve exact terminal\"}"
                    }
                }),
                responses::ev_completed("malformed-x-response"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("missing provider item_id"))
        );
        assert_eq!(
            response.requests().len(),
            1,
            "must not retry or dispatch locally"
        );
        assert_rollout_preserves_unknown_custom_item(&rollout_path, None, "x-call-missing-id")
            .await?;
    }

    {
        let fixture = ProviderRoutingFixture::new().await?;
        fixture.disable_image_generation()?;
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("undeclared-image-response"),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "image-undeclared",
                        "type": "image_generation_call",
                        "status": "completed",
                        "prompt": "Draw a forbidden circle.",
                        "result": TINY_PNG_DATA_URL
                    }
                }),
                responses::ev_completed("undeclared-image-response"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("was not declared"))
        );
        assert_eq!(
            response.requests().len(),
            1,
            "must not retry or dispatch locally"
        );
        assert!(!request_declares_tool(
            &response.single_request().body_json(),
            "image_generation",
        ));
        assert_rollout_omits_hosted_item(&rollout_path, "image-undeclared").await?;
    }

    {
        let fixture = ProviderRoutingFixture::new().await?;
        let arguments = serde_json::json!({
            "plan": [{
                "step": "must not dispatch after hosted ownership",
                "status": "completed"
            }]
        })
        .to_string();
        let response = mount_sse_once_match(
            &fixture.grok_server,
            header("authorization", "Bearer grok-test-key"),
            responses::sse(vec![
                responses::ev_response_created("changed-owner-response"),
                responses::ev_web_search_call_added_partial("shared-owner", "in_progress"),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "shared-owner",
                        "type": "function_call",
                        "call_id": "must-not-dispatch",
                        "name": "update_plan",
                        "arguments": arguments
                    }
                }),
                responses::ev_completed("changed-owner-response"),
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
        let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

        let completed = app
            .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(
            completed
                .turn
                .error
                .is_some_and(|error| error.message.contains("changed Tool Plan ownership"))
        );
        assert_eq!(response.requests().len(), 1, "must not retry");
        assert_rollout_tool_output_count(&rollout_path, "must-not-dispatch", /*expected*/ 0)
            .await?;
        assert_eq!(
            app.pending_notification_methods()
                .iter()
                .filter(|method| method.as_str() == "turn/plan/updated")
                .count(),
            0,
            "a hosted ownership mismatch must fail before local dispatch"
        );
    }

    Ok(())
}

#[tokio::test]
async fn grok_duplicate_ordinary_tool_terminal_dispatches_once() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let arguments = serde_json::json!({
        "plan": [{
            "step": "verify duplicate suppression",
            "status": "completed"
        }]
    })
    .to_string();
    let duplicate = serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "id": "ordinary-duplicate",
            "type": "function_call",
            "call_id": "ordinary-duplicate-call",
            "name": "update_plan",
            "arguments": arguments
        }
    });
    let responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("ordinary-duplicate-response"),
                duplicate.clone(),
                duplicate,
                responses::ev_completed("ordinary-duplicate-response"),
            ]),
            completion_sse("ordinary-duplicate-follow-up"),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    let rollout_path = started.thread.path.clone().context("Grok rollout path")?;

    let completed = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;

    assert_eq!(completed.turn.status, TurnStatus::Completed);
    assert_eq!(
        app.pending_notification_methods()
            .iter()
            .filter(|method| method.as_str() == "turn/plan/updated")
            .count(),
        1,
        "duplicate terminal items must dispatch the local tool exactly once"
    );
    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]
            .inputs_of_type("function_call_output")
            .iter()
            .filter(|item| {
                item.get("call_id").and_then(serde_json::Value::as_str)
                    == Some("ordinary-duplicate-call")
            })
            .count(),
        1,
        "duplicate terminal items must produce exactly one local tool output"
    );
    assert_rollout_tool_output_count(
        &rollout_path,
        "ordinary-duplicate-call",
        /*expected*/ 1,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn large_grok_image_result_uses_bounded_second_request_projection() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let large_result = "A".repeat(400_000);
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-large-image-response"),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "large-image-1",
                        "type": "image_generation_call",
                        "status": "completed",
                        "prompt": "Generate a large test artifact.",
                        "result": large_result
                    }
                }),
                responses::ev_assistant_message("grok-large-image-message", "Done"),
                responses::ev_completed("grok-large-image-response"),
            ]),
            completion_sse("grok-large-image-follow-up"),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    let first = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(first.turn.status, TurnStatus::Completed);
    assert_eq!(grok_responses.requests().len(), 1);

    let second = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(second.turn.status, TurnStatus::Completed);
    let requests = grok_responses.requests();
    assert_eq!(requests.len(), 2);
    let second_body = requests[1].body_json();
    let image_history = second_body["input"]
        .as_array()
        .context("second Grok request input should be an array")?
        .iter()
        .find(|item| item["id"] == "large-image-1")
        .context("second Grok request should retain bounded image history")?;
    assert_eq!(image_history["type"], "image_generation_call");
    assert_eq!(image_history["status"], "completed");
    assert_eq!(image_history["prompt"], "Generate a large test artifact.");
    assert!(
        image_history.get("result").is_none(),
        "request projection must omit the durable inline image result"
    );
    Ok(())
}

#[tokio::test]
async fn failed_grok_image_is_visible_and_replays_with_the_exact_terminal_shape() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-failed-image-response"),
                grok_image_failed_done_event("failed-image-1", "Failed image prompt"),
                responses::ev_assistant_message("grok-failed-image-message", "Unable to draw"),
                responses::ev_completed("grok-failed-image-response"),
            ]),
            completion_sse("grok-failed-image-follow-up"),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    let rollout_path = started
        .thread
        .path
        .clone()
        .context("failed Grok image thread must have a rollout path")?;

    let request_id = app
        .send_turn_start_request(turn_for_thread(&started.thread.id))
        .await?;
    let _: TurnStartResponse = timeout(DEFAULT_TIMEOUT, app.read_response(request_id)).await??;
    let failed_image = loop {
        let completed: ItemCompletedNotification =
            timeout(DEFAULT_TIMEOUT, app.read_notification("item/completed")).await??;
        if let ThreadItem::ImageGeneration(image) = completed.item {
            break image;
        }
    };
    assert_eq!(failed_image.id, "failed-image-1");
    assert_eq!(failed_image.status, "failed");
    assert_eq!(failed_image.prompt.as_deref(), Some("Failed image prompt"));
    assert_eq!(failed_image.result, "");
    assert!(failed_image.saved_path.is_none());
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let follow_up = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(follow_up.turn.status, TurnStatus::Completed);
    let requests = grok_responses.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        projected_grok_images(&requests[1].body_json()),
        vec![serde_json::json!({
            "id": "failed-image-1",
            "type": "image_generation_call",
            "status": "failed",
            "prompt": "Failed image prompt"
        })]
    );
    assert_rollout_preserves_failed_grok_image(
        &rollout_path,
        "failed-image-1",
        "Failed image prompt",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn multiple_grok_images_survive_cold_resume_and_fork_with_bounded_projection() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-image-history-seed"),
                grok_image_done_event(
                    "provider-image-first",
                    "First image prompt",
                    TINY_PNG_DATA_URL,
                ),
                grok_image_done_event(
                    "provider-image-second",
                    "Second image prompt",
                    TINY_PNG_DATA_URL,
                ),
                responses::ev_assistant_message("grok-image-history-message", "Done"),
                responses::ev_completed("grok-image-history-seed"),
            ]),
            completion_sse("grok-image-resumed-follow-up"),
            completion_sse("grok-image-fork-follow-up"),
        ],
    )
    .await;
    let mut primary = fixture.start_app().await?;
    let started = primary
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    let rollout_path = started
        .thread
        .path
        .clone()
        .context("Grok image thread must have a rollout path")?;

    materialize_thread(&mut primary, &started.thread.id).await?;
    timeout(DEFAULT_TIMEOUT, primary.shutdown_gracefully()).await??;

    let mut resumed_app = fixture.start_app().await?;
    let resume_id = resumed_app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        resumed_app.read_stream_until_response_message(RequestId::Integer(resume_id)),
    )
    .await??;
    let resumed = to_response::<ThreadResumeResponse>(response)?;
    materialize_thread(&mut resumed_app, &resumed.thread.id).await?;

    let fork_id = resumed_app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: resumed.thread.id,
            ..Default::default()
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        resumed_app.read_stream_until_response_message(RequestId::Integer(fork_id)),
    )
    .await??;
    let forked = to_response::<ThreadForkResponse>(response)?;
    let fork_rollout_path = forked
        .thread
        .path
        .clone()
        .context("forked Grok image thread must have a rollout path")?;
    materialize_thread(&mut resumed_app, &forked.thread.id).await?;
    timeout(DEFAULT_TIMEOUT, resumed_app.shutdown_gracefully()).await??;

    let requests = grok_responses.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests[1..] {
        let request_body = request.body_json();
        assert_eq!(
            projected_grok_images(&request_body),
            vec![
                serde_json::json!({
                    "id": "provider-image-first",
                    "type": "image_generation_call",
                    "status": "completed",
                    "prompt": "First image prompt"
                }),
                serde_json::json!({
                    "id": "provider-image-second",
                    "type": "image_generation_call",
                    "status": "completed",
                    "prompt": "Second image prompt"
                }),
            ]
        );
        assert_provider_item_order(
            &request_body,
            &["provider-image-first", "provider-image-second"],
        )?;
    }
    let expected_durable_images = [
        (
            "provider-image-first",
            "First image prompt",
            TINY_PNG_DATA_URL,
        ),
        (
            "provider-image-second",
            "Second image prompt",
            TINY_PNG_DATA_URL,
        ),
    ];
    assert_rollout_preserves_grok_images(&rollout_path, &expected_durable_images).await?;
    assert_rollout_preserves_grok_images(&fork_rollout_path, &expected_durable_images).await?;
    Ok(())
}

#[tokio::test]
async fn oversized_bounded_grok_image_projection_fails_before_second_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let oversized_prompt = "P".repeat(400_000);
    let grok_responses = mount_sse_once_match(
        &fixture.grok_server,
        header("authorization", "Bearer grok-test-key"),
        responses::sse(vec![
            responses::ev_response_created("grok-oversized-image-prompt-response"),
            serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "id": "oversized-image-prompt-1",
                    "type": "image_generation_call",
                    "status": "completed",
                    "prompt": oversized_prompt,
                    "result": "durable-result"
                }
            }),
            responses::ev_assistant_message("grok-oversized-image-prompt-message", "Done"),
            responses::ev_completed("grok-oversized-image-prompt-response"),
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

    let first = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(first.turn.status, TurnStatus::Completed);

    let second = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(second.turn.status, TurnStatus::Failed);
    assert_eq!(
        grok_responses.requests().len(),
        1,
        "a projected request that exceeds the model budget must not reach the Provider"
    );
    Ok(())
}

#[tokio::test]
async fn grok_model_without_context_window_fails_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    fixture.grok_server.reset().await;
    mount_grok_models_without_context_window(&fixture.grok_server, &["grok-model"]).await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    let turn = app
        .start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;

    assert_eq!(turn.turn.status, TurnStatus::Failed);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn incomplete_grok_hosted_items_fail_without_partial_persistence() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-incomplete-web-response"),
                responses::ev_web_search_call_added_partial("web-incomplete", "in_progress"),
                responses::ev_completed("grok-incomplete-web-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-incomplete-x-response"),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "x-incomplete",
                        "type": "custom_tool_call",
                        "status": "in_progress",
                        "call_id": "x-call-incomplete",
                        "name": "x_semantic_search",
                        "input": ""
                    }
                }),
                responses::ev_completed("grok-incomplete-x-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-incomplete-image-response"),
                serde_json::json!({
                    "type": "response.output_item.added",
                    "item": {
                        "id": "image-incomplete",
                        "type": "image_generation_call",
                        "status": "in_progress",
                        "result": null
                    }
                }),
                responses::ev_completed("grok-incomplete-image-response"),
            ]),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    for (prompt, item_id) in [
        ("Exercise incomplete Web lifecycle", "web-incomplete"),
        ("Exercise incomplete X lifecycle", "x-incomplete"),
        ("Exercise incomplete Image lifecycle", "image-incomplete"),
    ] {
        let started = app
            .start_thread(ThreadStartParams {
                model: Some("grok-model".to_string()),
                ..Default::default()
            })
            .await?;
        let rollout_path = started
            .thread
            .path
            .clone()
            .context("started Grok thread must have a rollout path")?;
        let completed = app
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: started.thread.id,
                input: vec![UserInput::Text {
                    text: prompt.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed);
        assert!(completed.turn.error.is_some());
        assert_rollout_omits_hosted_item(&rollout_path, item_id).await?;
    }
    assert_eq!(grok_responses.requests().len(), 3, "must not retry");
    Ok(())
}

#[tokio::test]
async fn turn_rejects_a_model_owned_by_another_provider_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;

    let request_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: started.thread.id.clone(),
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

    let response = mount_completion(&fixture.openai_server, "openai-after-rejected-settings").await;
    app.start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(
        response.single_request().body_json()["model"],
        "openai-model"
    );
    Ok(())
}

#[tokio::test]
async fn turn_allows_a_model_owned_by_the_bound_provider() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let response = mount_completion(&fixture.openai_server, "openai-same-provider").await;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;

    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: started.thread.id,
        input: vec![UserInput::Text {
            text: "Use another model from this provider".to_string(),
            text_elements: Vec::new(),
        }],
        model: Some("openai-model-2".to_string()),
        ..Default::default()
    })
    .await?;

    assert_eq!(
        response.single_request().body_json()["model"],
        "openai-model-2"
    );
    Ok(())
}

#[tokio::test]
async fn thread_settings_reject_a_model_owned_by_another_provider() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;

    let request_id = app
        .send_thread_settings_update_request(ThreadSettingsUpdateParams {
            thread_id: started.thread.id.clone(),
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

    let response = mount_completion(&fixture.openai_server, "openai-after-rejected-settings").await;
    app.start_turn_and_wait_for_completion(turn_for_thread(&started.thread.id))
        .await?;
    assert_eq!(
        response.single_request().body_json()["model"],
        "openai-model"
    );
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
async fn stock_single_provider_fork_keeps_unlisted_model_compatibility() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_stock_openai_only().await?;
    mount_completion(&fixture.openai_server, "openai-stock-fork-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;
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
    let ThreadForkResponse { thread, model, .. } = to_response(response)?;

    assert_eq!(thread.model_provider, "openai");
    assert_eq!(model, "unlisted-openai-model");
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn detached_review_inherits_the_parent_provider_binding() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            completion_sse("grok-review-seed"),
            completion_sse("grok-detached-review"),
        ],
    )
    .await;
    let openai_responses = mount_completion(&fixture.openai_server, "wrong-review-provider").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &started.thread.id).await?;

    let request_id = app
        .send_review_start_request(ReviewStartParams {
            thread_id: started.thread.id,
            target: ReviewTarget::Custom {
                instructions: "Review this thread".to_string(),
            },
            delivery: Some(ReviewDelivery::Detached),
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ReviewStartResponse {
        review_thread_id, ..
    } = to_response(response)?;
    let notification = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_matching_notification("detached review thread/started", |message| {
            message.method == "thread/started"
                && message.params.as_ref().is_some_and(|params| {
                    serde_json::from_value::<ThreadStartedNotification>(params.clone())
                        .is_ok_and(|started| started.thread.id == review_thread_id)
                })
        }),
    )
    .await??;
    let review_started: ThreadStartedNotification = serde_json::from_value(
        notification
            .params
            .context("thread/started must include params")?,
    )?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_matching_notification("detached review turn/completed", |message| {
            message.method == "turn/completed"
                && message.params.as_ref().is_some_and(|params| {
                    serde_json::from_value::<TurnCompletedNotification>(params.clone())
                        .is_ok_and(|completed| completed.thread_id == review_thread_id)
                })
        }),
    )
    .await??;

    assert_eq!(review_started.thread.model_provider, "grok");
    assert_eq!(grok_responses.requests().len(), 2);
    assert_eq!(
        grok_responses.requests()[1].body_json()["model"],
        "grok-model"
    );
    assert_eq!(openai_responses.requests().len(), 0);
    Ok(())
}

#[tokio::test]
async fn detached_review_rejects_a_cross_provider_review_model_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_review_model(Some("openai-model")).await?;
    mount_completion(&fixture.grok_server, "grok-review-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &started.thread.id).await?;
    let grok_request_count = received_responses_count(&fixture.grok_server).await?;

    let request_id = app
        .send_review_start_request(ReviewStartParams {
            thread_id: started.thread.id,
            target: ReviewTarget::Custom {
                instructions: "Review this thread".to_string(),
            },
            delivery: Some(ReviewDelivery::Detached),
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
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count
    );
    Ok(())
}

#[tokio::test]
async fn inline_review_rejects_a_cross_provider_review_model_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_review_model(Some("openai-model")).await?;
    mount_completion(&fixture.grok_server, "grok-inline-review-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &started.thread.id).await?;
    let grok_request_count = received_responses_count(&fixture.grok_server).await?;

    let request_id = app
        .send_review_start_request(ReviewStartParams {
            thread_id: started.thread.id,
            target: ReviewTarget::Custom {
                instructions: "Review this thread".to_string(),
            },
            delivery: Some(ReviewDelivery::Inline),
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
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count
    );
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
    let grok_request_count_before_resume = received_responses_count(&fixture.grok_server).await?;

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
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count_before_resume
    );
    Ok(())
}

#[tokio::test]
async fn cold_resume_reports_bound_model_removal_before_egress() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.grok_server, "grok-removal-seed").await;
    let mut primary = fixture.start_app().await?;
    let started = primary
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut primary, &started.thread.id).await?;
    timeout(DEFAULT_TIMEOUT, primary.shutdown_gracefully()).await??;

    std::fs::remove_file(
        provider_models_home(fixture.codex_home.path(), "grok").join("models_cache.json"),
    )?;
    fixture.openai_server.reset().await;
    fixture.grok_server.reset().await;
    mount_models_repeating(
        &fixture.openai_server,
        ModelsResponse {
            models: vec![remote_catalog_model("grok-model", "Reassigned Model")],
        },
    )
    .await;
    mount_grok_models_repeating(&fixture.grok_server, &[]).await;

    let mut secondary = fixture.start_app().await?;
    let request_id = secondary
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id,
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        secondary.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("ModelUnavailable"));
    assert!(!error.error.message.contains("belongs to provider"));
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn cold_resume_fails_closed_until_provider_registration_is_restored() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.grok_server, "grok-removed-provider-seed").await;
    let mut primary = fixture.start_app().await?;
    let started = primary
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut primary, &started.thread.id).await?;
    timeout(DEFAULT_TIMEOUT, primary.shutdown_gracefully()).await??;
    let grok_request_count_before_resume = received_responses_count(&fixture.grok_server).await?;

    let config_path = fixture.codex_home.path().join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    let registration = "model_provider_registrations = [\"openai\", \"grok\"]";
    assert!(config.contains(registration));
    let config = config.replacen(registration, "model_provider_registrations = []", 1);
    std::fs::write(&config_path, config)?;

    let mut secondary = fixture.start_app().await?;
    let request_id = secondary
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        secondary.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("ProviderUnavailable"));
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count_before_resume
    );

    timeout(DEFAULT_TIMEOUT, secondary.shutdown_gracefully()).await??;
    let config = std::fs::read_to_string(&config_path)?;
    let config = config.replacen("model_provider_registrations = []", registration, 1);
    std::fs::write(config_path, config)?;

    let mut restored = fixture.start_app().await?;
    let request_id = restored
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id,
            ..Default::default()
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        restored.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let resumed = to_response::<ThreadResumeResponse>(response)?;
    assert_eq!(resumed.thread.model_provider, "grok");
    assert_eq!(resumed.model, "grok-model");
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count_before_resume
    );
    Ok(())
}

#[tokio::test]
async fn running_resume_rejects_a_conflicting_provider_binding() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    mount_completion(&fixture.openai_server, "openai-running-resume-seed").await;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;
    materialize_thread(&mut app, &started.thread.id).await?;
    let openai_request_count_before_resume =
        received_responses_count(&fixture.openai_server).await?;
    let grok_request_count_before_resume = received_responses_count(&fixture.grok_server).await?;

    let request_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id,
            model: Some("grok-model".to_string()),
            model_provider: Some("grok".to_string()),
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
    assert_eq!(
        received_responses_count(&fixture.openai_server).await?,
        openai_request_count_before_resume
    );
    assert_eq!(
        received_responses_count(&fixture.grok_server).await?,
        grok_request_count_before_resume
    );
    Ok(())
}

#[tokio::test]
async fn raw_history_resume_rejects_an_unverifiable_provider_binding() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let mut app = fixture.start_app().await?;

    let request_id = app
        .send_thread_resume_request(ThreadResumeParams {
            // thread_id is intentionally ignored by the public raw-history API.
            thread_id: "ignored-raw-history-id".to_string(),
            history: Some(vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Unbound provider history".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }]),
            model: Some("grok-model".to_string()),
            model_provider: Some("grok".to_string()),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("history"));
    assert!(error.error.message.contains("provider binding"));
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

fn request_declares_tool(body: &serde_json::Value, tool_type: &str) -> bool {
    body["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["type"].as_str() == Some(tool_type))
    })
}

async fn assert_rollout_omits_hosted_item(
    rollout_path: &std::path::Path,
    item_id: &str,
) -> Result<()> {
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    assert!(history.history.iter().all(|item| {
        !matches!(
            item,
            RolloutItem::ResponseItem(response_item)
                if response_item.id().is_some_and(|id| id.as_str() == item_id)
        )
    }));
    Ok(())
}

async fn assert_rollout_preserves_unknown_custom_item(
    rollout_path: &std::path::Path,
    item_id: Option<&str>,
    call_id: &str,
) -> Result<()> {
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    assert_eq!(
        history
            .history
            .iter()
            .filter(|item| {
                let RolloutItem::ResponseItem(ResponseItem::CustomToolCall {
                    id,
                    call_id: persisted_call_id,
                    ..
                }) = item
                else {
                    return false;
                };
                persisted_call_id == call_id
                    && match (item_id, id.as_ref()) {
                        (Some(item_id), Some(id)) => id.as_str() == item_id,
                        (None, None) => true,
                        _ => false,
                    }
            })
            .count(),
        1,
        "unknown provider custom output must remain exact durable evidence"
    );
    assert!(history.history.iter().all(|item| !matches!(
        item,
        RolloutItem::ResponseItem(ResponseItem::CustomToolCallOutput {
            call_id: persisted_call_id,
            ..
        }) if persisted_call_id == call_id
    )));
    Ok(())
}

async fn assert_rollout_tool_output_count(
    rollout_path: &std::path::Path,
    call_id: &str,
    expected: usize,
) -> Result<()> {
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    assert_eq!(
        history
            .history
            .iter()
            .filter(|item| matches!(
                item,
                RolloutItem::ResponseItem(ResponseItem::FunctionCallOutput {
                    call_id: persisted_call_id,
                    ..
                }) if persisted_call_id == call_id
            ))
            .count(),
        expected,
        "unexpected durable tool output count for {call_id}"
    );
    Ok(())
}

fn grok_image_done_event(item_id: &str, prompt: &str, result: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "id": item_id,
            "type": "image_generation_call",
            "status": "completed",
            "prompt": prompt,
            "result": result
        }
    })
}

fn grok_image_failed_done_event(item_id: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "id": item_id,
            "type": "image_generation_call",
            "status": "failed",
            "prompt": prompt
        }
    })
}

fn projected_grok_images(request_body: &serde_json::Value) -> Vec<serde_json::Value> {
    request_body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "image_generation_call")
        .cloned()
        .collect()
}

fn assert_provider_item_order(
    request_body: &serde_json::Value,
    expected_ids: &[&str],
) -> Result<()> {
    let input = request_body["input"]
        .as_array()
        .context("Grok request input should be an array")?;
    let positions = expected_ids
        .iter()
        .map(|expected_id| {
            input
                .iter()
                .position(|item| item["id"].as_str() == Some(expected_id))
                .with_context(|| format!("Grok request is missing provider item `{expected_id}`"))
        })
        .collect::<Result<Vec<_>>>()?;
    anyhow::ensure!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "Grok request changed provider item order: {positions:?}"
    );
    Ok(())
}

async fn assert_rollout_preserves_grok_images(
    rollout_path: &std::path::Path,
    expected: &[(&str, &str, &str)],
) -> Result<()> {
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    let actual = history
        .history
        .iter()
        .filter_map(|item| {
            let RolloutItem::ResponseItem(ResponseItem::GrokImageGenerationCall {
                id: Some(id),
                status,
                prompt: Some(prompt),
                result: Some(result),
                ..
            }) = item
            else {
                return None;
            };
            Some((
                id.as_str(),
                status.as_str(),
                prompt.as_str(),
                result.as_str(),
            ))
        })
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .map(|(id, prompt, result)| (*id, "completed", *prompt, *result))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    Ok(())
}

async fn assert_rollout_preserves_failed_grok_image(
    rollout_path: &std::path::Path,
    expected_id: &str,
    expected_prompt: &str,
) -> Result<()> {
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    assert_eq!(
        history
            .history
            .iter()
            .filter(|item| matches!(
                item,
                RolloutItem::ResponseItem(ResponseItem::GrokImageGenerationCall {
                    id: Some(id),
                    status,
                    prompt: Some(prompt),
                    result: None,
                    ..
                }) if id.as_str() == expected_id
                    && status == "failed"
                    && prompt == expected_prompt
            ))
            .count(),
        1,
        "failed Grok image terminal evidence must remain durable exactly once"
    );
    Ok(())
}

async fn mount_grok_models_repeating(server: &MockServer, model_ids: &[&str]) {
    mount_grok_models_with_backend_search(server, model_ids, true).await;
}

async fn mount_grok_models_with_backend_search(
    server: &MockServer,
    model_ids: &[&str],
    supports_backend_search: bool,
) {
    let data = model_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "owned_by": "xai",
                "context_window": 272000,
                "supports_backend_search": supports_backend_search,
            })
        })
        .collect::<Vec<_>>();
    Mock::given(method("GET"))
        .and(path_regex(".*/models$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({"object": "list", "data": data})),
        )
        .mount(server)
        .await;
}

async fn mount_grok_models_without_context_window(server: &MockServer, model_ids: &[&str]) {
    let data = model_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "owned_by": "xai",
                "supports_backend_search": true,
            })
        })
        .collect::<Vec<_>>();
    Mock::given(method("GET"))
        .and(path_regex(".*/models$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({"object": "list", "data": data})),
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

async fn mount_completion(server: &MockServer, response_id: &str) -> responses::ResponseMock {
    mount_sse_once_match(server, method("POST"), completion_sse(response_id)).await
}

fn completion_sse(response_id: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(response_id),
        responses::ev_assistant_message(&format!("{response_id}-message"), "Seed response"),
        responses::ev_completed(response_id),
    ])
}

async fn materialize_thread(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    app.start_turn_and_wait_for_completion(turn_for_thread(thread_id))
        .await?;
    Ok(())
}

fn turn_for_thread(thread_id: &str) -> TurnStartParams {
    TurnStartParams {
        thread_id: thread_id.to_string(),
        input: vec![UserInput::Text {
            text: "Persist this thread".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    }
}
