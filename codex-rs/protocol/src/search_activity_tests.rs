use super::*;
use crate::legacy_events::HasLegacyEvent;
use crate::protocol::EventMsg;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn search_activity_round_trip_does_not_fabricate_legacy_web_events() {
    for (kind, kind_wire) in [
        (SearchActivityKind::Web, "web"),
        (SearchActivityKind::X, "x"),
    ] {
        for (state, wire) in [
            (SearchActivityState::Running, "running"),
            (SearchActivityState::Completed, "completed"),
            (SearchActivityState::Cleared, "cleared"),
        ] {
            let payload = SearchActivityEvent {
                attempt_id: 7,
                output_index: 1,
                item_id: "web".into(),
                kind,
                state,
            };
            let event = EventMsg::SearchActivity(payload.clone());
            let serialized = serde_json::to_value(&event).unwrap();
            assert_eq!(
                serialized,
                json!({"type":"search_activity","attempt_id":7,
            "output_index":1,"item_id":"web","kind":kind_wire,"state":wire})
            );
            let EventMsg::SearchActivity(restored) = serde_json::from_value(serialized).unwrap()
            else {
                panic!("activity round trip changed event kind");
            };
            assert_eq!(restored, payload);
            assert!(
                event
                    .as_legacy_events(/*show_raw_agent_reasoning*/ false)
                    .is_empty()
            );
            assert!(
                event
                    .as_legacy_events(/*show_raw_agent_reasoning*/ true)
                    .is_empty()
            );
        }
    }
}
