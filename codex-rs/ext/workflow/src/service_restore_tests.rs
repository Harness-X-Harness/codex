use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use crate::journal::WORKFLOW_ERROR_INFLIGHT_INTERRUPTED;
use crate::persist::load_workflow_document;
use crate::run::WorkflowStatus;
use codex_protocol::ThreadId;

#[tokio::test]
async fn restore_persists_fail_closed_orphan_and_releases_occupancy() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(95);
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    first
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start");
    first
        .mutate_run(thread_id, |run| {
            run.mark_pending_yield_started();
            Ok(())
        })
        .await
        .expect("mark orphan");
    drop(first);

    let restored = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let run = restored
        .get_run(thread_id)
        .await
        .expect("get")
        .expect("run");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some(WORKFLOW_ERROR_INFLIGHT_INTERRUPTED)
    );
    assert!(!run.pending_yield_started);
    assert!(!run.occupies_idle());

    let disk = load_workflow_document(dir.path(), &thread_id.to_string())
        .expect("load disk")
        .expect("persisted");
    let persisted: crate::run::WorkflowRun = serde_json::from_slice(&disk).expect("parse disk");
    assert_eq!(persisted.status, WorkflowStatus::Failed);
    assert_eq!(
        persisted.error.as_deref(),
        Some(WORKFLOW_ERROR_INFLIGHT_INTERRUPTED)
    );
}
