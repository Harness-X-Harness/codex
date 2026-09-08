use pretty_assertions::assert_eq;

use codex_extension_api::EngineOccupant;
use codex_extension_api::EngineSlot;
use codex_extension_api::ExtensionData;
use codex_extension_api::engine_slot;

#[test]
fn later_claim_waits_until_release() {
    let slot = EngineSlot::default();
    assert!(slot.try_claim(EngineOccupant::GoalHow));
    assert!(!slot.try_claim(EngineOccupant::Workflow));
    assert_eq!(slot.occupant(), Some(EngineOccupant::GoalHow));
    assert!(slot.release(EngineOccupant::GoalHow));
    assert!(slot.try_claim(EngineOccupant::Workflow));
    assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
}

#[test]
fn same_occupant_may_reclaim() {
    let slot = EngineSlot::default();
    assert!(slot.try_claim(EngineOccupant::Workflow));
    assert!(slot.try_claim(EngineOccupant::Workflow));
    assert!(!slot.release(EngineOccupant::GoalHow));
    assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
}

#[test]
fn thread_store_shares_one_slot() {
    let data = ExtensionData::new("thread-1");
    let first = engine_slot(&data);
    assert!(first.try_claim(EngineOccupant::GoalHow));
    let second = engine_slot(&data);
    assert!(!second.try_claim(EngineOccupant::Workflow));
}
