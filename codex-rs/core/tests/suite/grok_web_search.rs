#![allow(clippy::unwrap_used)]

use codex_model_provider_info::WireApi;
use codex_protocol::config_types::WebSearchConfig;
use codex_protocol::config_types::WebSearchFilters;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(key) || object.values().any(|child| contains_key(child, key))
        }
        Value::Array(items) => items.iter().any(|child| contains_key(child, key)),
        _ => false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_live_web_search_omits_external_web_access() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let sse = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_completed("resp-1"),
    ]);
    let resp_mock = responses::mount_sse_once(&server, sse).await;

    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        // Display name is not a dialect selector. Grok is selected only
        // through wire_api = grok_responses.
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

    test.submit_turn_with_permission_profile(
        "hello live grok web search",
        PermissionProfile::read_only(),
    )
    .await
    .expect("submit turn");

    let body = resp_mock.single_request().body_json();
    let tools = body["tools"]
        .as_array()
        .expect("Grok request should include tools");
    let web_search = tools
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search"))
        .expect("Grok should still advertise hosted web_search");
    let x_search = tools
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("x_search"));

    assert_eq!(web_search, &json!({"type": "web_search"}));
    assert_eq!(x_search, Some(&json!({"type": "x_search"})));
    assert!(
        !contains_key(&body, "external_web_access"),
        "Grok Responses rejects Argument not supported: external_web_access: {body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_emits_web_search_excluded_domains_from_stock_config() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let sse = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_completed("resp-1"),
    ]);
    let resp_mock = responses::mount_sse_once(&server, sse).await;

    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.model_provider.name = "xAI".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
        config
            .web_search_mode
            .set(WebSearchMode::Live)
            .expect("test web_search_mode should satisfy constraints");
        config.web_search_config = Some(WebSearchConfig {
            filters: Some(WebSearchFilters {
                allowed_domains: None,
                excluded_domains: Some(vec!["en.wikipedia.org".to_string()]),
            }),
            user_location: None,
            search_context_size: None,
        });
    });
    let test = builder
        .build_with_auto_env(&server)
        .await
        .expect("create Grok Codex conversation");

    test.submit_turn_with_permission_profile(
        "hello grok excluded domains",
        PermissionProfile::read_only(),
    )
    .await
    .expect("submit turn");

    let body = resp_mock.single_request().body_json();
    let tools = body["tools"]
        .as_array()
        .expect("Grok request should include tools");
    let web_search = tools
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search"))
        .expect("Grok should advertise hosted web_search");

    assert_eq!(
        web_search,
        &json!({
            "type": "web_search",
            "filters": {"excluded_domains": ["en.wikipedia.org"]}
        })
    );
}
