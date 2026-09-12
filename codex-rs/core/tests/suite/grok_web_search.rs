//! Grok-specific web search contracts. Kept apart from the stock `web_search`
//! suite so upstream edits to that file never collide with the Grok graft.

use codex_model_provider_info::WireApi;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn find_web_search_tool(body: &Value) -> &Value {
    body["tools"]
        .as_array()
        .expect("request body should include tools array")
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search"))
        .expect("tools should include a web_search tool")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_web_search_uses_bare_live_declaration_and_replays_stock_history() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let responses = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-grok-search"),
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "web_search_call",
                        "id": "ws-grok-search",
                        "status": "completed",
                        "action": {
                            "type": "search",
                            "query": "current UTC date",
                            "sources": [{"type": "url", "url": "https://example.com"}],
                        },
                    },
                }),
                ev_assistant_message("msg-grok-search", "search completed"),
                responses::ev_completed("resp-grok-search"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-grok-follow-up"),
                ev_assistant_message("msg-grok-follow-up", "replay completed"),
                responses::ev_completed("resp-grok-follow-up"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex().with_model("grok-4.6").with_config(|config| {
        config.model_provider_id = "grok".to_string();
        config.model_provider.name = "Grok".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
        config
            .web_search_mode
            .set(WebSearchMode::Cached)
            .expect("test web search mode should satisfy constraints");
    });
    let test = builder
        .build(&server)
        .await
        .expect("create test Grok conversation");

    test.submit_turn_with_permission_profile("search the web", PermissionProfile::read_only())
        .await
        .expect("submit Grok search turn");
    test.submit_turn_with_permission_profile(
        "continue with the prior result",
        PermissionProfile::read_only(),
    )
    .await
    .expect("submit Grok follow-up turn");

    let requests = responses.requests();
    let search_request = requests
        .iter()
        .find(|request| request.body_contains_text("search the web"))
        .expect("search turn should reach Grok");
    let follow_up_request = requests
        .iter()
        .find(|request| request.body_contains_text("continue with the prior result"))
        .expect("follow-up turn should reach Grok");
    for request in [search_request, follow_up_request] {
        assert_eq!(
            find_web_search_tool(&request.body_json()),
            &json!({"type": "web_search"})
        );
    }

    let follow_up = follow_up_request.body_json();
    let replayed_search = follow_up["input"]
        .as_array()
        .expect("follow-up should include canonical history")
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("web_search_call"))
        .expect("completed search item should be replayed");
    assert_eq!(
        replayed_search,
        &json!({
            "type": "web_search_call",
            "status": "completed",
            "action": {"type": "search", "query": "current UTC date"},
        }),
    );
}
