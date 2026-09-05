use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::WorkflowAdvance;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;

fn yield_then_complete() -> &'static str {
    r#"ask("Compile the crate."); complete();"#
}

#[test]
fn start_complete_without_yield() {
    let run = WorkflowRun::start(ThreadId::from_u128(1), "complete();").expect("start");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.pending_instruction, None);
    assert!(run.display_steps().is_empty());
}

#[test]
fn advance_resumes_after_ask_then_completes() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(2), yield_then_complete()).expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
    assert!(!run.pending_yield_started);
    run.mark_pending_yield_started();
    assert!(run.pending_yield_started);
    assert_eq!(
        run.advance_with_reply("compiled".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.served_replies, vec!["compiled".to_string()]);
    assert!(!run.occupies_idle());
}

#[test]
fn active_run_occupies_idle() {
    let run = WorkflowRun::start(ThreadId::from_u128(8), yield_then_complete()).expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert!(run.occupies_idle());
}

#[test]
fn stop_and_resume_are_host_owned() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(3), yield_then_complete()).expect("start");
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
}

#[tokio::test]
async fn service_persists_across_instances() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(4);
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let started = first
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    assert_eq!(started.name, "workflow");
    assert_eq!(started.status, WorkflowStatus::Active);
    let second = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = second.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(loaded.run_id, started.run_id);
    assert_eq!(loaded.status, WorkflowStatus::Active);
    let advanced = second.advance_run(thread_id).await.expect("advance");
    assert_eq!(advanced.status, WorkflowStatus::Complete);
}

#[tokio::test]
async fn starting_while_active_is_rejected() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(5);
    service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let err = service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect_err("second start");
    assert!(err.to_string().contains("already active"));
}

#[tokio::test]
async fn starting_after_complete_replaces_the_run() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(6);
    let completed = service
        .start_run(thread_id, "complete();")
        .await
        .expect("complete");
    assert_eq!(completed.status, WorkflowStatus::Complete);
    let replaced = service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("restart");
    assert_eq!(replaced.status, WorkflowStatus::Active);
    assert_ne!(replaced.run_id, completed.run_id);
}

#[tokio::test]
async fn markdown_source_is_rejected_by_the_service() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let err = service
        .start_run(
            ThreadId::from_u128(7),
            "# Ship\n\n## Build\nCompile the crate.\n",
        )
        .await
        .expect_err("markdown");
    assert!(
        err.to_string().contains("not valid Rhai"),
        "unexpected error: {err}"
    );
}
