use pretty_assertions::assert_eq;

use super::WorkflowAdvance;
use super::WorkflowRun;
use super::WorkflowStatus;
use crate::journal::ContinuationKind;
use crate::journal::HostCallResult;
use crate::journal::WORKFLOW_ERROR_REPLAY_DIVERGED;
use codex_protocol::ThreadId;

#[test]
fn queued_pause_resume_completes_without_a_second_resume() {
    let mut run =
        WorkflowRun::start(ThreadId::from_u128(80), "pause(); complete();").expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.pending_kind, Some(ContinuationKind::Pause));
    assert!(!run.pending_resume_intent);
    run.park().expect("park");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    assert!(run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Completed));
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert!(!run.pending_resume_intent);
    assert_eq!(run.continuations.len(), 1);
    assert_eq!(run.continuations[0].kind, ContinuationKind::Pause);
    assert_eq!(run.continuations[0].result, HostCallResult::success(""));
}

#[test]
fn queued_await_user_resume_completes_without_a_second_resume() {
    let mut run =
        WorkflowRun::start(ThreadId::from_u128(81), "await_user(); complete();").expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.pending_kind, Some(ContinuationKind::AwaitUser));
    run.park().expect("park");
    assert!(run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Completed));
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.continuations.len(), 1);
    assert_eq!(run.continuations[0].kind, ContinuationKind::AwaitUser);
    assert_eq!(run.continuations[0].result, HostCallResult::success(""));
}

#[test]
fn queued_pause_resume_still_fails_closed_on_replay_divergence() {
    let mut run =
        WorkflowRun::start(ThreadId::from_u128(82), "pause(); complete();").expect("start");
    run.park().expect("park");
    run.source = "await_user(); complete();".to_string();
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Failed));
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some(WORKFLOW_ERROR_REPLAY_DIVERGED));
    assert!(!run.pending_resume_intent);
    assert!(!run.occupies_idle());
}

#[test]
fn parked_stopped_ask_does_not_get_an_empty_success() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(83),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("start");
    run.stop().expect("stop");
    run.park().expect("park");
    assert!(!run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Yielded));
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
    assert!(run.continuations.is_empty());
}

#[test]
fn parked_stopped_agent_does_not_write_ok_true() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(84),
        r#"let r = agent("Say ok."); if r.ok { complete(); }"#,
    )
    .expect("start");
    run.stop().expect("stop");
    run.park().expect("park");
    assert!(!run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Yielded));
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("Say ok."));
    assert!(run.continuations.is_empty());
}

#[test]
fn parked_stopped_spawn_does_not_write_ok_true() {
    let mut run = WorkflowRun::start_with_spawn(
        ThreadId::from_u128(85),
        r#"
            let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
            if r.ok { complete(); }
        "#,
        crate::engine::SpawnBinding::Available,
    )
    .expect("start");
    run.stop().expect("stop");
    run.park().expect("park");
    assert!(!run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Yielded));
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("Say ok."));
    assert_eq!(run.pending_spawn_task_name.as_deref(), Some("review"));
    assert!(run.continuations.is_empty());
}

#[test]
fn queued_start_activate_still_yields_the_first_host_work() {
    let mut run = WorkflowRun::queue(
        ThreadId::from_u128(86),
        r#"ask("Compile the crate."); complete();"#,
    )
    .expect("queue");
    assert!(!run.pending_resume_intent);
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Yielded));
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
    assert!(run.continuations.is_empty());
    assert!(!run.pending_resume_intent);
}
