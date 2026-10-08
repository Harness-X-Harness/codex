use super::*;
use codex_protocol::SearchActivityEvent;
use codex_protocol::SearchActivityKind;
use codex_protocol::SearchActivityState;

#[test]
fn search_activity_is_not_durable_in_either_history_mode() {
    for state in [
        SearchActivityState::Running,
        SearchActivityState::Completed,
        SearchActivityState::Cleared,
    ] {
        let item = RolloutItem::EventMsg(EventMsg::SearchActivity(SearchActivityEvent {
            attempt_id: 1,
            output_index: 2,
            item_id: "web".into(),
            kind: SearchActivityKind::Web,
            state,
        }));
        for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
            assert!(!is_persisted_rollout_item(&item, mode));
        }
    }
}
