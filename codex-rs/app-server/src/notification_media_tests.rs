use codex_app_server_protocol::SearchActivityKind;
use codex_app_server_protocol::SearchActivityNotification;
use codex_app_server_protocol::SearchActivityState;
use codex_app_server_protocol::ServerNotification;
use pretty_assertions::assert_eq;

use super::without_notification_media;

#[test]
fn search_activity_preserves_identity_and_state_when_media_is_omitted() {
    for state in [
        SearchActivityState::Running,
        SearchActivityState::Completed,
        SearchActivityState::Cleared,
    ] {
        let notification = ServerNotification::SearchActivity(SearchActivityNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
            attempt_id: 3,
            output_index: 4,
            item_id: "search-5".to_string(),
            kind: SearchActivityKind::Web,
            state,
        });
        let expected = serde_json::to_value(&notification).unwrap();

        let filtered = without_notification_media(notification);

        assert_eq!(serde_json::to_value(filtered).unwrap(), expected);
    }
}
