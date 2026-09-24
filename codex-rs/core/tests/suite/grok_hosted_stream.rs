#![allow(clippy::unwrap_used)]

use codex_core::TurnInputRequest;
use codex_model_provider_info::WireApi;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const RS_ID: &str = "rs_hosted";
const MSG_ID: &str = "msg_hosted";
const WS0_ID: &str = "ws_hosted-0";
const WS1_ID: &str = "ws_hosted-1";
const TCO0_ID: &str = "tco_hosted-0";
const TCO1_ID: &str = "tco_hosted-1";
const MESSAGE_TEXT: &str = "Let me look it up.";

fn added(index: u64, item: Value) -> Value {
    json!({"type": "response.output_item.added", "output_index": index, "item": item})
}

fn done(index: u64, item: Value) -> Value {
    json!({"type": "response.output_item.done", "output_index": index, "item": item})
}

fn delta(text: &str) -> Value {
    json!({
        "type": "response.output_text.delta",
        "output_index": 1,
        "item_id": MSG_ID,
        "delta": text
    })
}

fn reasoning(id: &str, summary: Value, blob: Option<&str>) -> Value {
    let mut item = json!({"id": id, "type": "reasoning", "summary": summary});
    if let Some(blob) = blob {
        item["encrypted_content"] = json!(blob);
    }
    item
}

fn web_search(id: &str, action: Value) -> Value {
    json!({"id": id, "type": "web_search_call", "status": "completed", "action": action})
}

fn interleaved_hosted_sse() -> String {
    let summary = json!([{"type": "summary_text", "text": "plan the search"}]);
    responses::sse(vec![
        ev_response_created("resp-1"),
        added(0, reasoning(RS_ID, summary.clone(), None)),
        done(0, reasoning(RS_ID, summary, Some("enc-rs-0"))),
        added(
            1,
            json!({"content": [], "id": MSG_ID, "role": "assistant", "type": "message"}),
        ),
        delta("Let"),
        delta(" me"),
        delta(" look"),
        added(
            2,
            web_search(
                WS0_ID,
                json!({"type": "search", "query": "example query", "sources": []}),
            ),
        ),
        done(
            2,
            web_search(
                WS0_ID,
                json!({
                    "type": "search",
                    "query": "example query",
                    "sources": [{"type": "url", "url": "https://example.com/"}]
                }),
            ),
        ),
        added(
            3,
            web_search(
                WS1_ID,
                json!({"type": "open_page", "url": "https://example.com/"}),
            ),
        ),
        done(
            3,
            web_search(
                WS1_ID,
                json!({"type": "open_page", "url": "https://example.com/"}),
            ),
        ),
        added(4, reasoning(TCO0_ID, json!([]), Some("enc-tco-0"))),
        done(4, reasoning(TCO0_ID, json!([]), Some("enc-tco-0"))),
        added(5, reasoning(TCO1_ID, json!([]), Some("enc-tco-1"))),
        done(5, reasoning(TCO1_ID, json!([]), Some("enc-tco-1"))),
        added(6, reasoning(RS_ID, json!([]), None)),
        done(6, reasoning(RS_ID, json!([]), Some("enc-rs-6"))),
        delta(" it"),
        delta(" up"),
        delta("."),
        json!({"type": "response.output_text.done", "output_index": 1, "item_id": MSG_ID, "text": MESSAGE_TEXT}),
        done(
            1,
            json!({
                "content": [{"type": "output_text", "text": MESSAGE_TEXT}],
                "id": MSG_ID,
                "role": "assistant",
                "type": "message"
            }),
        ),
        ev_completed("resp-1"),
    ])
}

fn item_type_and_id(item: &Value) -> (String, String) {
    let ty = item.get("type").and_then(Value::as_str).unwrap_or_default();
    let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
    (ty.to_string(), id.to_string())
}

fn replayed_history(input: Vec<Value>) -> Vec<Value> {
    input
        .into_iter()
        .filter(|item| item["role"].as_str() == Some("assistant") || item["type"] != "message")
        .collect()
}

const EXPECTED_TYPES: &[&str] = &[
    "reasoning",
    "message",
    "web_search_call",
    "web_search_call",
    "reasoning",
    "reasoning",
    "reasoning",
];
const EXPECTED_REPLAY: &[(&str, &str)] = &[
    ("reasoning", RS_ID),
    ("message", MSG_ID),
    ("web_search_call", WS0_ID),
    ("web_search_call", WS1_ID),
    ("reasoning", TCO0_ID),
    ("reasoning", TCO1_ID),
    ("reasoning", RS_ID),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_interleaved_hosted_stream_completes_and_replays_in_canonical_order() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let second = responses::sse(vec![
        ev_response_created("resp-2"),
        ev_assistant_message("msg-2", "ok"),
        ev_completed("resp-2"),
    ]);
    let mock = responses::mount_sse_sequence(&server, vec![interleaved_hosted_sse(), second]).await;

    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.model_provider.name = "xAI".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
        config
            .web_search_mode
            .set(WebSearchMode::Live)
            .expect("test web_search_mode should satisfy constraints");
    });
    let test = builder
        .build_with_auto_env(&server)
        .await
        .expect("create Grok Codex conversation");

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "search the site".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("submit interleaved turn");

    let mut deltas = Vec::new();
    let mut completed_types = Vec::new();
    let mut completed_message = None;
    loop {
        match wait_for_event(&test.codex, |_| true).await {
            EventMsg::AgentMessageContentDelta(event) => deltas.push(event.delta),
            EventMsg::ItemCompleted(ItemCompletedEvent { item, .. }) => {
                let item_type = match &item {
                    TurnItem::Reasoning(_) => Some("reasoning"),
                    TurnItem::AgentMessage(_) => Some("message"),
                    TurnItem::WebSearch(_) => Some("web_search_call"),
                    _ => None,
                };
                if let Some(item_type) = item_type {
                    completed_types.push(item_type);
                }
                if let TurnItem::AgentMessage(item) = item {
                    completed_message = Some(item);
                }
            }
            EventMsg::Error(err) => panic!("turn error: {}", err.message),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    let completed_message = completed_message.expect("completed agent message");
    let completed_text: String = completed_message
        .content
        .iter()
        .map(|entry| match entry {
            AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect();
    assert_eq!(deltas.concat(), completed_text);
    assert_eq!(completed_text, MESSAGE_TEXT);
    assert_eq!(completed_types, EXPECTED_TYPES);
    assert_eq!(mock.requests().len(), 1);

    test.submit_turn_with_permission_profile("continue", PermissionProfile::read_only())
        .await
        .expect("submit follow-up turn");

    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let replayed = replayed_history(requests[1].input());
    assert_eq!(
        replayed.iter().map(item_type_and_id).collect::<Vec<_>>(),
        EXPECTED_REPLAY
            .iter()
            .map(|(ty, id)| (ty.to_string(), id.to_string()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        replayed
            .iter()
            .filter(|item| item["type"] == "web_search_call")
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            json!({"type": "web_search_call", "id": WS0_ID, "action": {"type": "search", "query": "example query"}}),
            json!({"type": "web_search_call", "id": WS1_ID, "action": {"type": "open_page", "url": "https://example.com/"}}),
        ]
    );
    assert_eq!(
        replayed
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .map(|item| (
                item["id"].as_str(),
                item["encrypted_content"].as_str(),
                item.get("content").is_some()
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some(RS_ID), Some("enc-rs-0"), false),
            (Some(TCO0_ID), Some("enc-tco-0"), false),
            (Some(TCO1_ID), Some("enc-tco-1"), false),
            (Some(RS_ID), Some("enc-rs-6"), false),
        ]
    );
}
