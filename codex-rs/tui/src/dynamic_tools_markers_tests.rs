use super::tool_marker;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::WebSearchItem;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn hosted_web_marker_preserves_canonical_identity() {
    let item = ThreadItemPayload::WebSearch(WebSearchItem {
        id: "web-item".into(),
        query: "query 日本語".repeat(2_000),
        action: None,
        results: None,
    });
    assert_eq!(
        tool_marker(&item, "turn-1"),
        Some(json!({
            "id": "web-item", "turnId": "turn-1", "type": "webSearch",
            "name": "webSearch", "status": null
        }))
    );
}
