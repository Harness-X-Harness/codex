use pretty_assertions::assert_eq;

use super::WorkflowAdvance;
use super::WorkflowRun;
use super::WorkflowStatus;
use crate::journal::HostCallResult;
use crate::journal::WORKFLOW_ERROR_INFLIGHT_INTERRUPTED;
use codex_protocol::ThreadId;

fn round_trip(run: &WorkflowRun) -> WorkflowRun {
    let bytes = serde_json::to_vec(run).expect("serialize");
    let mut restored: WorkflowRun = serde_json::from_slice(&bytes).expect("deserialize");
    restored.prepare_restored().expect("restore");
    restored
}

#[test]
fn restore_fails_closed_for_active_orphan_in_flight() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(90),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.mark_pending_yield_started();
    let restored = round_trip(&run);
    assert_eq!(restored.status, WorkflowStatus::Failed);
    assert_eq!(
        restored.error.as_deref(),
        Some(WORKFLOW_ERROR_INFLIGHT_INTERRUPTED)
    );
    assert!(!restored.pending_yield_started);
    assert!(!restored.occupies_idle());
    assert_eq!(restored.continuations, run.continuations);
}

#[test]
fn restore_fails_closed_for_paused_orphan_in_flight() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(91),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.mark_pending_yield_started();
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    let restored = round_trip(&run);
    assert_eq!(restored.status, WorkflowStatus::Failed);
    assert_eq!(
        restored.error.as_deref(),
        Some(WORKFLOW_ERROR_INFLIGHT_INTERRUPTED)
    );
    assert!(!restored.pending_yield_started);
    assert!(!restored.occupies_idle());
}

#[test]
fn restore_fails_closed_for_waiting_orphan_in_flight() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(92),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.mark_pending_yield_started();
    run.yield_occupancy().expect("yield occupancy");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    let restored = round_trip(&run);
    assert_eq!(restored.status, WorkflowStatus::Failed);
    assert_eq!(
        restored.error.as_deref(),
        Some(WORKFLOW_ERROR_INFLIGHT_INTERRUPTED)
    );
    assert!(!restored.occupies_idle());
}

#[test]
fn restore_keeps_active_run_schedulable_when_no_host_work_started() {
    let run = WorkflowRun::start(
        ThreadId::from_u128(93),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    assert!(!run.pending_yield_started);
    let restored = round_trip(&run);
    assert_eq!(restored.status, WorkflowStatus::Active);
    assert!(!restored.pending_yield_started);
    assert!(restored.occupies_idle());
    assert_eq!(
        restored.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
}

#[test]
fn restore_clears_orphan_flag_on_complete_without_rewriting_result() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(94),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    assert_eq!(
        run.advance_with_outcome(HostCallResult::success("compiled".to_string())),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
    run.mark_pending_yield_started();
    let restored = round_trip(&run);
    assert_eq!(restored.status, WorkflowStatus::Complete);
    assert_eq!(restored.error, None);
    assert!(!restored.pending_yield_started);
    assert_eq!(restored.continuations, run.continuations);
    assert_eq!(restored.result, run.result);
}
