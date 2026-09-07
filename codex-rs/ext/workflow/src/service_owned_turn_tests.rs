use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use crate::journal::HostCallResult;
use crate::run::WorkflowStatus;
use codex_protocol::ThreadId;

#[tokio::test]
async fn foreign_turn_result_is_not_applied_to_an_owned_yield() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(60);
    service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start");
    service
        .mutate_run(thread_id, |run| {
            run.mark_pending_yield_started();
            Ok(())
        })
        .await
        .expect("mark started");
    service.in_flight.remember(thread_id, "turn-a".to_string());
    service.stop_run(thread_id).await.expect("stop");

    let ignored = service
        .finish_owned_host_turn(thread_id, "turn-b", HostCallResult::success("ok"))
        .await
        .expect("foreign")
        .expect("run");
    assert_eq!(ignored.status, WorkflowStatus::Paused);
    assert!(ignored.pending_yield_started);
    assert!(ignored.continuations.is_empty());

    let applied = service
        .finish_owned_host_turn(thread_id, "turn-a", HostCallResult::success("ok"))
        .await
        .expect("owned")
        .expect("run");
    assert_eq!(applied.status, WorkflowStatus::Complete);
    assert!(!applied.pending_yield_started);
    assert_eq!(applied.continuations.len(), 1);
}
