use super::tool_marker;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::XSearchItem;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn hosted_x_marker_preserves_identity_without_local_execution_metadata() {
    let item = ThreadItem::XSearch(XSearchItem {
        id: "x-item".into(),
        call_id: "provider-call".into(),
        name: "x_keyword_search".into(),
        input: "private query 日本語".repeat(2_000),
    });
    assert_eq!(
        tool_marker(&item, "turn-1"),
        Some(json!({
            "id": "x-item", "turnId": "turn-1", "type": "xSearch",
            "name": "x_keyword_search", "status": null
        }))
    );
}
