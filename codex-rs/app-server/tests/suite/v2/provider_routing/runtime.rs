use super::*;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadItem;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Notify;
use wiremock::Request;
use wiremock::Respond;

const SUBAGENT_PARENT_PROMPT: &str = "Spawn one child reviewer";
const SUBAGENT_CHILD_PROMPT: &str = "Review the Grok provider binding";

#[tokio::test]
async fn unified_home_keeps_chatgpt_subscription_account_visible() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_implicit_openai_default().await?;
    let mut app = fixture.start_app().await?;
    let request_id = app
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(
        to_response::<GetAccountResponse>(response)?,
        GetAccountResponse {
            account: Some(Account::Chatgpt {
                email: None,
                plan_type: codex_protocol::account::PlanType::Pro,
            }),
            requires_openai_auth: true,
        }
    );
    Ok(())
}

#[derive(Clone, Default)]
struct FederatedSubagentResponder {
    child_requests: Arc<Mutex<Vec<Value>>>,
    child_request_seen: Arc<Notify>,
}

impl Respond for FederatedSubagentResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body)
            .expect("request compression is disabled for provider routing tests");
        let input = body["input"]
            .as_array()
            .expect("Responses input should be an array");
        if input
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
        {
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("grok-parent-follow-up"),
                responses::ev_assistant_message("grok-parent-message", "Parent complete"),
                responses::ev_completed("grok-parent-follow-up"),
            ]));
        }

        let body_text = body.to_string();
        if body_text.contains(SUBAGENT_CHILD_PROMPT) {
            self.child_requests
                .lock()
                .expect("child request log should not be poisoned")
                .push(body);
            self.child_request_seen.notify_one();
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("grok-child"),
                responses::ev_assistant_message("grok-child-message", "Child complete"),
                responses::ev_completed("grok-child"),
            ]));
        }

        if body_text.contains(SUBAGENT_PARENT_PROMPT) {
            let wire_name = body["tools"]
                .as_array()
                .and_then(|tools| {
                    tools.iter().find_map(|tool| {
                        let properties = tool
                            .pointer("/parameters/properties")
                            .and_then(Value::as_object)?;
                        (properties.contains_key("message") && properties.contains_key("task_name"))
                            .then(|| tool.get("name").and_then(Value::as_str))
                            .flatten()
                    })
                })
                .expect("Grok Tool Plan should expose the stable spawn-agent function");
            let arguments = json!({
                "message": SUBAGENT_CHILD_PROMPT,
                "task_name": "reviewer"
            })
            .to_string();
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("grok-parent"),
                responses::ev_function_call("grok-spawn", wire_name, &arguments),
                responses::ev_completed("grok-parent"),
            ]));
        }

        ResponseTemplate::new(400).set_body_string("unexpected provider routing request")
    }
}

#[tokio::test]
async fn unified_home_defaults_to_chatgpt_subscription_without_a_provider_override() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_implicit_openai_default().await?;
    let openai_responses = mount_sse_once_match(
        &fixture.openai_server,
        header("authorization", "Bearer chatgpt-access-token"),
        completion_sse("openai-default"),
    )
    .await;
    let mut app = fixture.start_app().await?;
    let started = app.start_thread(ThreadStartParams::default()).await?;

    assert_eq!(started.model, "openai-model");
    assert_eq!(started.model_provider, "openai");
    materialize_thread(&mut app, &started.thread.id).await?;

    assert_eq!(openai_responses.requests().len(), 1);
    assert_eq!(received_responses_count(&fixture.grok_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn openai_and_grok_threads_run_concurrently_with_provider_scoped_auth() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_implicit_openai_default().await?;
    let openai_responses = mount_sse_once_match(
        &fixture.openai_server,
        header("authorization", "Bearer chatgpt-access-token"),
        completion_sse("openai-concurrent"),
    )
    .await;
    let grok_responses = mount_sse_once_match(
        &fixture.grok_server,
        header("authorization", "Bearer grok-test-key"),
        completion_sse("grok-concurrent"),
    )
    .await;
    let mut app = fixture.start_app().await?;
    let openai_thread = app.start_thread(ThreadStartParams::default()).await?;
    let grok_thread = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    app.send_turn_start_request(turn_for_thread(&openai_thread.thread.id))
        .await?;
    app.send_turn_start_request(turn_for_thread(&grok_thread.thread.id))
        .await?;

    let mut completed_threads = Vec::new();
    for _ in 0..2 {
        let completed: TurnCompletedNotification =
            timeout(DEFAULT_TIMEOUT, app.read_notification("turn/completed")).await??;
        completed_threads.push(completed.thread_id);
    }
    completed_threads.sort();
    let mut expected_threads = vec![openai_thread.thread.id, grok_thread.thread.id];
    expected_threads.sort();

    assert_eq!(completed_threads, expected_threads);
    assert_eq!(openai_responses.requests().len(), 1);
    assert_eq!(grok_responses.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn grok_subagent_inherits_parent_provider_binding_and_auth() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_implicit_openai_default().await?;
    let responder = FederatedSubagentResponder::default();
    let child_requests = Arc::clone(&responder.child_requests);
    let child_request_seen = Arc::clone(&responder.child_request_seen);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(header("authorization", "Bearer grok-test-key"))
        .respond_with(responder)
        .mount(&fixture.grok_server)
        .await;
    let mut app = fixture.start_app().await?;
    let grok_thread = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;

    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: grok_thread.thread.id,
        input: vec![UserInput::Text {
            text: SUBAGENT_PARENT_PROMPT.to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    })
    .await?;

    if child_requests
        .lock()
        .expect("child request log should not be poisoned")
        .is_empty()
    {
        timeout(DEFAULT_TIMEOUT, child_request_seen.notified()).await?;
    }

    let child_models = child_requests
        .lock()
        .expect("child request log should not be poisoned")
        .iter()
        .map(|request| request["model"].clone())
        .collect::<Vec<_>>();
    assert_eq!(child_models, vec![json!("grok-model")]);
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn grok_compaction_and_follow_up_keep_provider_binding_and_auth() -> Result<()> {
    let fixture = ProviderRoutingFixture::with_implicit_openai_default().await?;
    let grok_responses = responses::mount_sse_sequence(
        &fixture.grok_server,
        vec![
            completion_sse("grok-before-compact"),
            completion_sse("grok-compact-summary"),
            completion_sse("grok-after-compact"),
        ],
    )
    .await;
    let mut app = fixture.start_app().await?;
    let grok_thread = app
        .start_thread(ThreadStartParams {
            model: Some("grok-model".to_string()),
            ..Default::default()
        })
        .await?;
    materialize_thread(&mut app, &grok_thread.thread.id).await?;

    let compact_request_id = app
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: grok_thread.thread.id.clone(),
        })
        .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(compact_request_id)),
    )
    .await??;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_matching_notification(
            "Grok context compaction completion",
            |message| {
                message.method == "item/completed"
                    && message.params.as_ref().is_some_and(|params| {
                        serde_json::from_value::<ItemCompletedNotification>(params.clone())
                            .is_ok_and(|completed| {
                                completed.thread_id == grok_thread.thread.id
                                    && matches!(
                                        completed.item,
                                        ThreadItem::ContextCompaction { .. }
                                    )
                            })
                    })
            },
        ),
    )
    .await??;
    materialize_thread(&mut app, &grok_thread.thread.id).await?;

    let requests = grok_responses.requests();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert_eq!(
            request.header("authorization").as_deref(),
            Some("Bearer grok-test-key")
        );
        assert_eq!(request.body_json()["model"], "grok-model");
    }
    assert_eq!(received_responses_count(&fixture.openai_server).await?, 0);
    Ok(())
}
