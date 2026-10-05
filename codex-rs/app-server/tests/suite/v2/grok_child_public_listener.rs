//! Public listener evidence used by the shipped V2 child oracle. This deliberately
//! uses the stock Responses mock: Grok model-driven tool wire is owned by C7.
//!
//! LocalAgentControl admits the initial input before notify_thread_created
//! (core/src/agent/control/spawn.rs). This fixture makes no child subscription or
//! resume request. It proves that the real auto-attached listener delivers that
//! initial input, with the parent's raw opt-in, before the child's bound result.
//! It does not claim that raw emission always precedes listener attachment.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::write_models_cache;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::protocol::SubAgentSource;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::timeout;

const PARENT_INPUT: &str = "delegate the listener witness";
const CHILD_INPUT: &str = "return the listener witness result";
const CHILD_RESULT: &str = "listener child completed";
const SPAWN: &str = "listener-spawn";
const WAIT: &str = "listener-wait";

fn is_listener_child_request(request: &wiremock::Request) -> bool {
    let body: Value = serde_json::from_slice(&request.body).expect("captured JSON request body");
    body["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["type"] == "agent_message" && item["recipient"] == "/root/listener_child"
        })
    })
}

#[test_case(true; "parent raw opt in inherited")]
#[test_case(false; "parent raw opt out retained")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_child_input_reaches_inherited_public_listener(raw_enabled: bool) -> Result<()> {
    let server = responses::start_mock_server().await;
    let mut spawn = responses::ev_function_call_with_namespace(
        SPAWN,
        "collaboration",
        "spawn_agent",
        &json!({"task_name": "listener_child", "message": CHILD_INPUT}).to_string(),
    );
    // The real V2 handler uses this existing wire marker to select readable
    // input. Opaque encrypted input cannot prove the Live nonce exclusion.
    spawn["item"]["encrypted_function_args"] = json!([]);
    responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body = String::from_utf8_lossy(&request.body);
            body.contains(PARENT_INPUT)
                && !body.contains(SPAWN)
                && !is_listener_child_request(request)
        },
        responses::sse(vec![
            responses::ev_response_created("parent-spawn"),
            spawn,
            responses::ev_completed("parent-spawn"),
        ]),
    )
    .await;
    let child_requests = responses::mount_sse_once_match(
        &server,
        is_listener_child_request,
        responses::sse(vec![
            responses::ev_response_created("child-response"),
            responses::ev_assistant_message("raw-child-result", CHILD_RESULT),
            responses::ev_completed("child-response"),
        ]),
    )
    .await;
    responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body = String::from_utf8_lossy(&request.body);
            body.contains(SPAWN) && !body.contains(WAIT) && !is_listener_child_request(request)
        },
        responses::sse(vec![
            responses::ev_response_created("parent-wait"),
            // A completion may already have been drained before this call.
            // Keep the stock minimum wait inside the whole observation budget.
            responses::ev_function_call_with_namespace(
                WAIT,
                "collaboration",
                "wait_agent",
                r#"{"timeout_ms":10000}"#,
            ),
            responses::ev_completed("parent-wait"),
        ]),
    )
    .await;
    responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body = String::from_utf8_lossy(&request.body);
            body.contains(WAIT) && !is_listener_child_request(request)
        },
        responses::sse(vec![
            responses::ev_response_created("parent-result"),
            responses::ev_assistant_message("parent-result", "delegation complete"),
            responses::ev_completed("parent-result"),
        ]),
    )
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model("gpt-5.4")
        .with_extra_config("[features.multi_agent_v2]\nenabled = true")
        .write(home.path())?;
    write_models_cache(home.path()).await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let start = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            experimental_raw_events: raw_enabled,
            ..Default::default()
        })
        .await?;
    let parent: ThreadStartResponse = app.read_response(start).await?;
    let request_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: parent.thread.id.clone(),
            input: vec![UserInput::Text {
                text: PARENT_INPUT.into(),
                text_elements: vec![],
            }],
            ..Default::default()
        })
        .await?;
    // Read every notification, including those preceding the turn/start RPC
    // response. No helper discards the early child queue.
    let (parent_turn, events) = timeout(Duration::from_secs(30), async {
        let mut events = Vec::new();
        let mut parent_turn = None;
        let mut parent_done = false;
        let mut child_done = false;
        while parent_turn.is_none() || !parent_done || !child_done {
            match app.read_next_message().await? {
                JSONRPCMessage::Response(response) => {
                    assert_eq!(response.id, RequestId::Integer(request_id));
                    let response: TurnStartResponse = app_test_support::to_response(response)?;
                    parent_turn = Some(response.turn.id);
                }
                JSONRPCMessage::Notification(notification) => {
                    let params = notification.params.unwrap_or(Value::Null);
                    if notification.method == "turn/completed" {
                        assert_eq!(params["turn"]["status"], "completed");
                        assert_eq!(params["turn"]["error"], Value::Null);
                        if params["threadId"] == parent.thread.id {
                            parent_done = true;
                        } else {
                            child_done = true;
                        }
                    }
                    events.push((notification.method, params));
                    assert!(events.len() <= 512, "public listener evidence budget");
                }
                other => anyhow::bail!("unexpected public message: {other:?}"),
            }
        }
        anyhow::Ok((parent_turn.expect("observed parent turn response"), events))
    })
    .await??;
    let read = app
        .send_thread_read_request(ThreadReadParams {
            thread_id: parent.thread.id.clone(),
            include_turns: true,
        })
        .await?;
    let parent_history: ThreadReadResponse = app.read_response(read).await?;
    let delegation = parent_history
        .thread
        .turns
        .iter()
        .find(|turn| turn.id == parent_turn)
        .expect("durable parent delegation turn");
    let (child_id, child_path) = delegation
        .items
        .iter()
        .find_map(|item| match item {
            ThreadItem::SubAgentActivity {
                kind: SubAgentActivityKind::Started,
                agent_thread_id,
                agent_path,
                ..
            } => Some((agent_thread_id.clone(), agent_path.clone())),
            _ => None,
        })
        .expect("real public V2 started activity");
    let read = app
        .send_thread_read_request(ThreadReadParams {
            thread_id: child_id.clone(),
            include_turns: true,
        })
        .await?;
    let child: ThreadReadResponse = app.read_response(read).await?;
    assert_eq!(
        child.thread.parent_thread_id.as_ref(),
        Some(&parent.thread.id)
    );
    assert_eq!(
        child.thread.forked_from_id.as_ref(),
        Some(&parent.thread.id)
    );
    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        agent_path: Some(source_path),
        ..
    }) = child.thread.source
    else {
        anyhow::bail!("child source path missing")
    };
    assert_eq!(source_path.to_string(), child_path);
    let child_turn = child.thread.turns.iter().find(|turn| turn.items.iter().any(|item| matches!(item, ThreadItem::AgentMessage { text, .. } if text == CHILD_RESULT))).expect("durable child result");
    assert_eq!(child_turn.status, TurnStatus::Completed);
    assert_eq!(child_turn.error, None);
    let child_events: Vec<_> = events
        .iter()
        .filter(|(_, params)| params["threadId"] == child_id)
        .collect();
    let raw: Vec<_> = child_events
        .iter()
        .enumerate()
        .filter(|(_, (method, _))| method == "rawResponseItem/completed")
        .collect();
    let start_at = child_events
        .iter()
        .position(|(method, params)| {
            method == "turn/started" && params["turn"]["id"] == child_turn.id
        })
        .expect("child start");
    let completed_at = child_events
        .iter()
        .position(|(method, params)| {
            method == "turn/completed" && params["turn"]["id"] == child_turn.id
        })
        .expect("child completion");
    let outbound = child_requests
        .requests()
        .into_iter()
        .find(|request| request.header("thread-id").as_deref() == Some(child_id.as_str()))
        .expect("actual child inference");
    let consumed = outbound.inputs_of_type("agent_message");
    assert_eq!(consumed.len(), 1);
    assert_eq!(
        consumed[0]["content"],
        json!([{"type": "input_text", "text": format!(
            "Message Type: NEW_TASK\nTask name: {child_path}\nSender: /root\nPayload:\n{CHILD_INPUT}"
        )}])
    );
    assert_eq!(consumed[0]["recipient"], child_path);
    if raw_enabled {
        let (input_at, (_, input)) = raw
            .iter()
            .find(|(_, (_, params))| params["item"]["type"] == "agent_message")
            .expect("initial input survives inherited attachment");
        let (result_at, (_, result)) = raw
            .iter()
            .find(|(_, (_, params))| {
                params["item"]["type"] == "message" && params["item"]["role"] == "assistant"
            })
            .expect("raw child result");
        assert!(start_at < *input_at && input_at < result_at && *result_at < completed_at);
        assert_eq!(input["turnId"], child_turn.id);
        assert_eq!(input["item"]["content"], consumed[0]["content"]);
        assert_eq!(input["item"]["author"], consumed[0]["author"]);
        assert_eq!(input["item"]["recipient"], child_path);
        assert_eq!(result["turnId"], child_turn.id);
        assert_eq!(
            result["item"]["content"],
            json!([{"type": "output_text", "text": CHILD_RESULT}])
        );
        // The durable history builder owns reconstructed item IDs. The contract
        // binds child, turn, and exact result text, not raw/durable item-ID equality.
    } else {
        assert!(
            raw.is_empty(),
            "child must inherit the parent's raw opt-out"
        );
    }
    assert!(app.shutdown_gracefully().await?.success());
    Ok(())
}
