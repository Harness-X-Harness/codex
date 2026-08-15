use anyhow::Result;
use codex_features::Feature;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_grok_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::configure_grok_test_provider;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::sleep;
use wiremock::Mock;
use wiremock::Request;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const PARENT_PROMPT: &str = "spawn a reviewer with collaboration";
const CHILD_PROMPT: &str = "review the provider codec";

#[derive(Clone, Default)]
struct GrokSubagentResponder {
    child_requests: Arc<Mutex<Vec<Value>>>,
}

impl Respond for GrokSubagentResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&decoded_request_body(request))
            .expect("request body should be JSON");
        let input = body["input"]
            .as_array()
            .expect("request input should be an array");
        if input
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
        {
            return sse_response(sse(vec![
                ev_response_created("resp-parent-follow-up"),
                ev_assistant_message("msg-parent", "parent done"),
                ev_completed("resp-parent-follow-up"),
            ]));
        }

        let body_text = body.to_string();
        // A full-history child request contains the parent prompt too. Match
        // the more specific child prompt first so the mock does not replay the
        // parent's spawn response inside the child session.
        if body_text.contains(CHILD_PROMPT) {
            self.child_requests
                .lock()
                .expect("child request log should not be poisoned")
                .push(body);
            return sse_response(sse(vec![
                ev_response_created("resp-child"),
                ev_assistant_message("msg-child", "child done"),
                ev_completed("resp-child"),
            ]));
        }

        if body_text.contains(PARENT_PROMPT) {
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
                .expect("Grok Tool Plan should declare the spawn-agent function schema");
            let arguments = json!({
                "message": CHILD_PROMPT,
                "task_name": "reviewer"
            })
            .to_string();
            return sse_response(sse(vec![
                ev_response_created("resp-parent"),
                ev_function_call("spawn-1", wire_name, &arguments),
                ev_completed("resp-parent"),
            ]));
        }

        ResponseTemplate::new(400).set_body_string("unexpected Grok test request")
    }
}

fn decoded_request_body(request: &Request) -> Vec<u8> {
    let is_zstd = request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
        });
    if is_zstd {
        zstd::stream::decode_all(Cursor::new(&request.body))
            .expect("zstd request body should decode")
    } else {
        request.body.clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_subagent_first_turn_uses_standard_plaintext_model_input() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_grok_mock_server("koffing").await;
    let responder = GrokSubagentResponder::default();
    let child_requests = Arc::clone(&responder.child_requests);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let test = test_codex()
        // Use the deterministic test model fixture. The provider wire API, not
        // the model slug, selects the Grok dialect under test.
        .with_model("koffing")
        .with_config(|config| {
            configure_grok_test_provider(config);
            config
                .features
                .enable(Feature::Collab)
                .expect("Collab should be configurable");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("MultiAgentV2 should be configurable");
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn(PARENT_PROMPT).await?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let child_request = loop {
        if let Some(request) = child_requests
            .lock()
            .expect("child request log should not be poisoned")
            .first()
            .cloned()
        {
            break request;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("Grok child request was not sent");
        }
        sleep(Duration::from_millis(10)).await;
    };

    let child_input = child_request["input"]
        .as_array()
        .expect("child request input should be an array");
    assert_eq!(child_request["model"], "koffing");
    assert!(child_input.iter().all(|item| {
        item.get("type").and_then(Value::as_str) != Some("agent_message")
            && item.get("encrypted_content").is_none()
    }));
    let projected_message = child_input
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("message")
                && item.get("role").and_then(Value::as_str) == Some("user")
                && item.to_string().contains(CHILD_PROMPT)
        })
        .expect("child prompt should be projected as a standard user message");
    assert_eq!(
        projected_message.get("type").and_then(Value::as_str),
        Some("message")
    );

    Ok(())
}
