#![allow(clippy::unwrap_used)]

use codex_model_provider_info::WireApi;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn find_x_search_tool(body: &Value) -> &Value {
    body["tools"]
        .as_array()
        .expect("request body should include tools array")
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("x_search"))
        .expect("tools should include an x_search tool")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_x_search_is_provider_hosted_and_replays_canonical_history() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let responses = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-grok-x-search"),
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "id": "ctc-grok-x-search",
                        "status": "completed",
                        "call_id": "xs-grok-x-search",
                        "name": "x_keyword_search",
                        "input": "{\"query\":\"current AI news\"}",
                    },
                }),
                ev_assistant_message("msg-grok-x-search", "X search completed"),
                responses::ev_completed("resp-grok-x-search"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-grok-x-follow-up"),
                ev_assistant_message("msg-grok-x-follow-up", "history accepted"),
                responses::ev_completed("resp-grok-x-follow-up"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex().with_model("grok-4.6").with_config(|config| {
        config.model_provider_id = "grok".to_string();
        config.model_provider.name = "Grok".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
    });
    let test = builder
        .build(&server)
        .await
        .expect("create test Grok conversation");

    test.submit_turn_with_permission_profile("search X", PermissionProfile::read_only())
        .await
        .expect("submit Grok X Search turn");
    test.submit_turn_with_permission_profile(
        "continue with the prior X result",
        PermissionProfile::read_only(),
    )
    .await
    .expect("submit Grok X Search follow-up turn");

    let requests = responses.requests();
    assert_eq!(
        requests.len(),
        2,
        "provider-hosted X Search must not create a local tool follow-up"
    );
    for request in &requests {
        assert_eq!(
            find_x_search_tool(&request.body_json()),
            &json!({"type": "x_search"}),
        );
    }

    let follow_up = requests[1].body_json();
    let input = follow_up["input"]
        .as_array()
        .expect("follow-up should include canonical history");
    let replayed_x_call = input
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("custom_tool_call")
                && item.get("call_id").and_then(Value::as_str) == Some("xs-grok-x-search")
        })
        .expect("completed X Search item should be replayed");
    // Stock request preparation strips unprefixed server item IDs before replay.
    assert_eq!(
        replayed_x_call,
        &json!({
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": "xs-grok-x-search",
            "name": "x_keyword_search",
            "input": "{\"query\":\"current AI news\"}",
        }),
    );
    assert!(
        input.iter().all(|item| {
            !matches!(
                item.get("type").and_then(Value::as_str),
                Some("custom_tool_call_output") | Some("function_call_output")
            )
        }),
        "provider-hosted X Search must not create a local tool result"
    );
}
