#![allow(clippy::unwrap_used)]

use codex_model_provider_info::WireApi;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const CALL_ID: &str = "xs_call-1";
const ITEM_ID: &str = "xs_1";
const HOSTED_NAME: &str = "x_keyword_search";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_hosted_x_search_follow_up_replays_unpaired_custom_tool_call() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let first = responses::sse(vec![
        ev_response_created("resp-1"),
        ev_assistant_message("msg-1", "searching X"),
        json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "custom_tool_call",
                "id": ITEM_ID,
                "call_id": CALL_ID,
                "name": HOSTED_NAME,
                "status": "completed",
                "input": "{\"query\":\"xai\"}"
            }
        }),
        ev_completed("resp-1"),
    ]);
    let second = responses::sse(vec![
        ev_response_created("resp-2"),
        ev_assistant_message("msg-2", "ok"),
        ev_completed("resp-2"),
    ]);
    let mock = responses::mount_sse_sequence(&server, vec![first, second]).await;

    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.model_provider.name = "xAI".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
    });
    let test = builder
        .build_with_auto_env(&server)
        .await
        .expect("create Grok Codex conversation");

    test.submit_turn_with_permission_profile("search X", PermissionProfile::read_only())
        .await
        .expect("hosted x_search turn");
    test.submit_turn_with_permission_profile("continue", PermissionProfile::read_only())
        .await
        .expect("follow-up turn after hosted x_search");

    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let replayed: Vec<Value> = requests[1]
        .input()
        .into_iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("custom_tool_call")
                || item.get("type").and_then(Value::as_str) == Some("custom_tool_call_output")
        })
        .collect();
    assert_eq!(
        replayed
            .iter()
            .map(|item| (
                item.get("type").and_then(Value::as_str).unwrap_or_default(),
                item.get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ))
            .collect::<Vec<_>>(),
        vec![("custom_tool_call", CALL_ID)]
    );
    assert_eq!(replayed[0]["name"], HOSTED_NAME);
    assert_eq!(replayed[0]["id"], ITEM_ID);
}
