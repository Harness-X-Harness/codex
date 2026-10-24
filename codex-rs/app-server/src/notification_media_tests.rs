use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ServerNotification;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::without_notification_media;

#[test]
fn hosted_web_preserves_identity_and_result_when_media_is_omitted() {
    let params: ItemCompletedNotification = serde_json::from_value(json!({
        "threadId":"thread-1", "turnId":"turn-2", "completedAtMs":1,
        "item":{"type":"webSearch", "id":"web-1", "query":"fixture query",
        "action":{"type":"search", "query":"fixture query", "queries":null},
        "results":[{"type":"provider_result", "extra":{"source":"fixture"}}]}
    }))
    .unwrap();
    let notification = ServerNotification::ItemCompleted(params);
    let expected = serde_json::to_value(&notification).unwrap();
    assert_eq!(
        serde_json::to_value(without_notification_media(notification)).unwrap(),
        expected
    );
}
