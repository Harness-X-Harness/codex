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

#[test]
fn search_activity_x_provenance_is_durable_but_never_a_legacy_web_call() {
    use codex_protocol::protocol::HasLegacyEvent;
    let item = codex_protocol::items::TurnItem::XSearch(codex_protocol::items::XSearchItem {
        id: "x".into(),
        call_id: "call".into(),
        name: "x_keyword_search".into(),
        input: "exact input\n".into(),
    });
    let completed = EventMsg::ItemCompleted(codex_protocol::protocol::ItemCompletedEvent {
        thread_id: codex_protocol::ThreadId::new(),
        turn_id: "turn".into(),
        item,
        started_at_ms: None,
        completed_at_ms: 1,
    });
    assert!(
        completed
            .as_legacy_events(/*show_raw_agent_reasoning*/ false)
            .is_empty()
    );
    for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        assert!(is_persisted_rollout_item(
            &RolloutItem::EventMsg(completed.clone()),
            mode
        ));
    }
}
