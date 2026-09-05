use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::WorkflowAdvance;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::parse_workflow_markdown;

fn sample_source() -> &'static str {
    "# Review PR\n\n## Gather\nRead the diff.\n\n## Write\nList bugs.\n"
}

#[test]
fn advance_moves_through_steps_then_completes() {
    let definition = parse_workflow_markdown(sample_source()).expect("parse");
    let mut run = WorkflowRun::start(ThreadId::from_u128(1), definition);
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.current_step().map(|step| step.id.as_str()),
        Some("gather")
    );
    assert_eq!(run.advance(), Ok(WorkflowAdvance::Advanced));
    assert_eq!(
        run.current_step().map(|step| step.id.as_str()),
        Some("write")
    );
    assert_eq!(run.advance(), Ok(WorkflowAdvance::Completed));
    assert_eq!(run.status, WorkflowStatus::Complete);
}

#[test]
fn stop_and_resume_are_host_owned() {
    let definition = parse_workflow_markdown(sample_source()).expect("parse");
    let mut run = WorkflowRun::start(ThreadId::from_u128(2), definition);
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
}

#[tokio::test]
async fn service_persists_across_instances() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(3);
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let started = first
        .start_run(thread_id, sample_source())
        .await
        .expect("start");
    assert_eq!(started.name, "Review PR");
    let second = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = second.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(loaded.run_id, started.run_id);
    assert_eq!(loaded.status, WorkflowStatus::Active);
    let advanced = second.advance_run(thread_id).await.expect("advance");
    assert_eq!(advanced.current_step_index, 1);
}

#[tokio::test]
async fn starting_while_active_is_rejected() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(4);
    service
        .start_run(thread_id, sample_source())
        .await
        .expect("start");
    let err = service
        .start_run(thread_id, sample_source())
        .await
        .expect_err("second start");
    assert!(err.to_string().contains("already active"));
}

#[tokio::test]
async fn starting_after_complete_replaces_the_run() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(5);
    service
        .start_run(thread_id, sample_source())
        .await
        .expect("start");
    service.advance_run(thread_id).await.expect("advance");
    let completed = service.advance_run(thread_id).await.expect("complete");
    assert_eq!(completed.status, WorkflowStatus::Complete);
    let replaced = service
        .start_run(thread_id, sample_source())
        .await
        .expect("restart");
    assert_eq!(replaced.status, WorkflowStatus::Active);
    assert_ne!(replaced.run_id, completed.run_id);
}
