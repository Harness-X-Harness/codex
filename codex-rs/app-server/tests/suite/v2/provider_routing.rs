use anyhow::Context;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::remote_catalog_model;
use app_test_support::to_response;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ReviewDelivery;
use codex_app_server_protocol::ReviewStartParams;
use codex_app_server_protocol::ReviewStartResponse;
use codex_app_server_protocol::ReviewTarget;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartedNotification;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_config::types::AuthCredentialsStoreMode;
use codex_core::RolloutRecorder;
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
        )
        .await
    }

    async fn with_implicit_openai_default() -> Result<Self> {
        Self::with_config(
            "", /*review_model*/ None, /*multi_agent_v2*/ false,
        )
        .await
    }

    async fn with_implicit_openai_default_and_multi_agent_v2() -> Result<Self> {
        Self::with_config("", /*review_model*/ None, /*multi_agent_v2*/ true).await
    }

    async fn with_config(
        default_selection: &str,
        review_model: Option<&str>,
        multi_agent_v2: bool,
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
        std::fs::write(
            codex_home.path().join("config.toml"),
            format!(
                r#"
{default_selection}{review_model}approval_policy = "never"
sandbox_mode = "read-only"
web_search = "live"
openai_base_url = "{}/v1"

[model_providers.grok]
name = "Grok"
base_url = "{}/v1"
env_key = "GROK_API_KEY"
wire_api = "grok_responses"
x_search = true

[features]
enable_request_compression = false
{multi_agent_v2}
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
                        "result": "data:image/png;base64,AAAA"
                    }
                }),
                serde_json::json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "image-1",
                        "type": "image_generation_call",
                        "status": "completed",
                        "prompt": "Draw a blue circle.",
                        "result": "data:image/png;base64,AAAA"
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
        app.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: started.thread.id.clone(),
            input: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
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
    }
    Ok(())
}

#[tokio::test]
async fn incomplete_grok_hosted_item_fails_without_partial_persistence() -> Result<()> {
    let fixture = ProviderRoutingFixture::new().await?;
    let grok_responses = mount_sse_once_match(
        &fixture.grok_server,
        header("authorization", "Bearer grok-test-key"),
        responses::sse(vec![
            responses::ev_response_created("grok-incomplete-response"),
            responses::ev_web_search_call_added_partial("web-incomplete", "in_progress"),
            responses::ev_completed("grok-incomplete-response"),
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
    let rollout_path = started
        .thread
        .path
        .clone()
        .context("started Grok thread must have a rollout path")?;

    let completed = app
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: started.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Exercise incomplete hosted lifecycle".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    assert!(completed.turn.error.is_some());
    assert_eq!(grok_responses.requests().len(), 1, "must not retry");

    let history = RolloutRecorder::get_rollout_history(&rollout_path).await?;
    let InitialHistory::Resumed(history) = history else {
        anyhow::bail!("expected materialized Grok rollout history");
    };
    assert!(
        history.history.iter().all(|item| !matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::WebSearchCall { id: Some(id), .. })
                if id.as_str() == "web-incomplete"
        )),
        "an incomplete hosted item must not enter durable history"
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
            thread_id: started.thread.id,
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

async fn mount_grok_models_repeating(server: &MockServer, model_ids: &[&str]) {
    let data = model_ids
        .iter()
        .map(|id| serde_json::json!({"id": id, "object": "model", "owned_by": "xai"}))
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
