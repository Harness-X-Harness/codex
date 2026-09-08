//! Grok-specific multi-agent (collaboration V2) contracts. Kept apart from the
//! stock `agent_execution` suite so upstream edits there never collide with the
//! Grok graft; the helpers below are intentionally private copies.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_model_provider_info::WireApi;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use wiremock::Mock;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const FIRST_PROMPT: &str = "spawn the first worker";
const FIRST_TASK: &str = "first worker task";
const CHILD_TASK_ENVELOPE: &str =
    "Message Type: NEW_TASK\nTask name: /root/first\nSender: /root\nPayload:\nfirst worker task";
const CHILD_COMPLETION_ENVELOPE: &str =
    "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/first\nPayload:\nworker completed";

/// Matches `text` against the JSON-encoded body so multi-line envelopes
/// (which serialize with escaped newlines) can be located.
fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    let json_fragment = serde_json::to_string(text)
        .expect("serialize text to JSON")
        .trim_matches('"')
        .to_string();
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .is_ok_and(|body| body.to_string().contains(&json_fragment))
}

fn has_function_call_output(request: &wiremock::Request, call_id: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(&request.body).is_ok_and(|body| {
        body.get("input")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("type").and_then(serde_json::Value::as_str)
                        == Some("function_call_output")
                        && item.get("call_id").and_then(serde_json::Value::as_str) == Some(call_id)
                })
            })
    })
}

fn flat_function_name(request: &wiremock::Request, canonical_label: &str) -> String {
    let body: serde_json::Value =
        serde_json::from_slice(&request.body).expect("request body should be JSON");
    body.get("tools")
        .and_then(serde_json::Value::as_array)
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.get("type").and_then(serde_json::Value::as_str) == Some("function")
                    && tool
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|description| description.contains(canonical_label))
            })
        })
        .and_then(|tool| tool.get("name"))
        .and_then(serde_json::Value::as_str)
        .expect("request should declare the canonical flat function")
        .to_string()
}

async fn mount_flat_function_call<M>(
    server: &wiremock::MockServer,
    matcher: M,
    canonical_label: &'static str,
    call_id: &'static str,
    response_id: &'static str,
    arguments: String,
) where
    M: wiremock::Match + Send + Sync + 'static,
{
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(matcher)
        .respond_with(move |request: &wiremock::Request| {
            let wire_name = flat_function_name(request, canonical_label);
            sse_response(sse(vec![
                ev_response_created(response_id),
                ev_function_call(call_id, &wire_name, &arguments),
                ev_completed(response_id),
            ]))
        })
        .up_to_n_times(1)
        .mount(server)
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_ultra_v2_full_history_is_gateway_compatible() -> Result<()> {
    let server = start_mock_server().await;
    let spawn_arguments = serde_json::to_string(&json!({
        "message": FIRST_TASK,
        "task_name": "first",
    }))?;
    mount_flat_function_call(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, FIRST_PROMPT)
                && !body_contains(request, FIRST_TASK)
                && !has_function_call_output(request, "grok-spawn-call")
        },
        "canonical `collaboration.spawn_agent` tool",
        "grok-spawn-call",
        "grok-root-response",
        spawn_arguments,
    )
    .await;
    mount_sse_once_match(
        &server,
        move |request: &wiremock::Request| {
            body_contains(request, FIRST_TASK)
                && body_contains(request, FIRST_PROMPT)
                && !has_function_call_output(request, "grok-spawn-call")
        },
        sse(vec![
            ev_response_created("resp-worker-grok-spawn-call"),
            ev_assistant_message("msg-worker-grok-spawn-call", "worker completed"),
            ev_completed("resp-worker-grok-spawn-call"),
        ]),
    )
    .await;
    mount_flat_function_call(
        &server,
        |request: &wiremock::Request| {
            has_function_call_output(request, "grok-spawn-call")
                && !has_function_call_output(request, "grok-wait-call")
        },
        "canonical `collaboration.wait_agent` tool",
        "grok-wait-call",
        "grok-root-wait",
        "{}".to_string(),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            has_function_call_output(request, "grok-wait-call")
                && body_contains(request, CHILD_COMPLETION_ENVELOPE)
        },
        sse(vec![
            ev_response_created("grok-root-complete"),
            ev_assistant_message("grok-root-message", "child completed"),
            ev_completed("grok-root-complete"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_model("grok-4.6").with_config(|config| {
        config.model_provider_id = "grok".to_string();
        config.model_provider.name = "Grok".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
        config
            .features
            .enable(Feature::Collab)
            .expect("test config should allow feature update");
        config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
    });
    let test = builder.build(&server).await?;
    let mut created_threads = test.thread_manager.subscribe_thread_created();

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: FIRST_PROMPT.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let child_id = tokio::time::timeout(Duration::from_secs(10), created_threads.recv()).await??;
    let child = test.thread_manager.get_thread(child_id).await?;
    wait_for_event(child.as_ref(), |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    wait_for_event(test.codex.as_ref(), |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let child_config = child.config_snapshot().await;
    assert_eq!(child_config.model_provider_id, "grok");
    assert_eq!(child_config.model, "grok-4.6");

    let requests = server.received_requests().await.expect("capture requests");
    assert!(
        requests
            .iter()
            .any(|request| body_contains(request, CHILD_TASK_ENVELOPE))
    );
    assert!(
        requests
            .iter()
            .any(|request| body_contains(request, CHILD_COMPLETION_ENVELOPE))
    );
    for request in requests {
        let body: serde_json::Value = serde_json::from_slice(&request.body)?;
        assert_eq!(body["model"], "grok-4.6");
        if !body_contains(&request, CHILD_TASK_ENVELOPE) {
            assert_eq!(body["reasoning"]["effort"], "xhigh");
        }
        assert!(
            !body["input"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "agent_message"))
        );
    }

    Ok(())
}
