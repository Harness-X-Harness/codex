use super::InFlightTurns;
use codex_protocol::ThreadId;

#[test]
fn owns_matches_recorded_turn_and_rejects_foreign_or_missing_ids() {
    let turns = InFlightTurns::default();
    let thread = ThreadId::from_u128(1);
    assert!(turns.owns(thread, None));
    assert!(turns.owns(thread, Some("turn-a")));

    turns.remember(thread, "turn-a".to_string());
    assert!(turns.owns(thread, Some("turn-a")));
    assert!(!turns.owns(thread, Some("turn-b")));
    assert!(!turns.owns(thread, None));

    turns.forget(thread);
    assert!(turns.owns(thread, Some("turn-b")));
    assert!(turns.owns(thread, None));
}
