use pretty_assertions::assert_eq;

use super::WorkflowAdvance;
use super::WorkflowRun;
use super::WorkflowStatus;
use crate::journal::HOST_ERROR_TURN_CANCELLED;
use crate::journal::HostCallResult;
use codex_protocol::ThreadId;

fn agent_then_ask() -> &'static str {
    r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "ok" { ask("next"); } else { complete(); }
    "#
}

#[test]
fn stop_without_started_host_turn_resumes() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(50),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    assert!(!run.pending_yield_started);
    run.stop().expect("stop");
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
}

#[test]
fn paused_run_rejects_advance_with_outcome() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(51),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.mark_pending_yield_started();
    run.stop().expect("stop");
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Err("workflow is not active".to_string())
    );
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert!(run.continuations.is_empty());
}

#[test]
fn late_success_after_stop_with_remaining_yield_stays_paused() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(52), agent_then_ask()).expect("start");
    run.mark_pending_yield_started();
    run.stop().expect("stop");
    run.apply_owned_host_result(HostCallResult::success("ok"))
        .expect("late success");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert!(!run.pending_yield_started);
    assert_eq!(run.pending_instruction.as_deref(), Some("next"));
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("next"));
}

#[test]
fn confirmed_cancel_while_paused_does_not_journal() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(53),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.mark_pending_yield_started();
    run.stop().expect("stop");
    run.apply_owned_host_result(HostCallResult::failure(HOST_ERROR_TURN_CANCELLED))
        .expect("cancel");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert!(!run.pending_yield_started);
    assert!(run.continuations.is_empty());
    run.resume().expect("resume");
    assert_eq!(run.advance(), Ok(WorkflowAdvance::Completed));
}
