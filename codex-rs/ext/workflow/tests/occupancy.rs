//! Transactional Workflow occupancy and restore reconciliation.

use pretty_assertions::assert_eq;
use std::sync::Arc;

use codex_extension_api::EngineOccupant;
use codex_extension_api::EngineSlot;
use codex_protocol::ThreadId;
use codex_workflow_extension::OwnershipEffect;
use codex_workflow_extension::WorkflowClaim;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::reconcile_workflow_ownership;

#[test]
fn dropped_new_claim_releases_the_slot() {
    let slot = Arc::new(EngineSlot::default());
    {
        let claim = WorkflowClaim::acquire(Some(Arc::clone(&slot)));
        assert!(claim.succeeded());
        assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
    }
    assert_eq!(slot.occupant(), None);
}

#[test]
fn committed_claim_keeps_the_slot() {
    let slot = Arc::new(EngineSlot::default());
    {
        let claim = WorkflowClaim::acquire(Some(Arc::clone(&slot)));
        assert!(claim.succeeded());
        claim.commit();
    }
    assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
}

#[test]
fn start_rollback_releases_a_reclaimed_phantom() {
    let slot = Arc::new(EngineSlot::default());
    assert!(slot.try_claim(EngineOccupant::Workflow));
    {
        let mut claim = WorkflowClaim::acquire(Some(Arc::clone(&slot)));
        assert!(claim.succeeded());
        claim.rollback_if_held();
    }
    assert_eq!(slot.occupant(), None);
}

#[test]
fn denied_claim_does_not_touch_goal_how() {
    let slot = Arc::new(EngineSlot::default());
    assert!(slot.try_claim(EngineOccupant::GoalHow));
    let claim = WorkflowClaim::acquire(Some(Arc::clone(&slot)));
    assert!(!claim.succeeded());
    assert_eq!(slot.occupant(), Some(EngineOccupant::GoalHow));
}

#[test]
fn vacuous_claim_without_a_slot_succeeds() {
    let claim = WorkflowClaim::acquire(None);
    assert!(claim.succeeded());
}

#[test]
fn reconcile_parks_active_run_when_goal_how_owns_the_slot() {
    let slot = EngineSlot::default();
    assert!(slot.try_claim(EngineOccupant::GoalHow));
    let mut run = WorkflowRun::start(ThreadId::from_u128(7), r#"ask("x"); complete();"#)
        .expect("active yield");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        reconcile_workflow_ownership(&slot, &mut run),
        OwnershipEffect::Parked
    );
    assert_eq!(run.status, WorkflowStatus::Waiting);
    assert_eq!(slot.occupant(), Some(EngineOccupant::GoalHow));
}

#[test]
fn reconcile_holds_active_run_on_an_empty_slot() {
    let slot = EngineSlot::default();
    let mut run = WorkflowRun::start(ThreadId::from_u128(8), r#"ask("x"); complete();"#)
        .expect("active yield");
    assert_eq!(
        reconcile_workflow_ownership(&slot, &mut run),
        OwnershipEffect::Held
    );
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
}

#[test]
fn activate_restores_a_parked_yield_without_re_eval() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(9), r#"ask("x"); complete();"#)
        .expect("active yield");
    let instruction = run.pending_instruction.clone();
    run.yield_occupancy().expect("park");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    run.activate().expect("activate");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction, instruction);
}
