use std::sync::Arc;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use super::WorkflowServiceError;
use crate::journal::HostCallResult;
use crate::journal::WORKFLOW_ERROR_HOST_RUNTIME;
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

#[tokio::test]
async fn remembered_turn_blocks_resume_before_pending_flag_is_persisted() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(61);
    service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start");
    service.stop_run(thread_id).await.expect("stop");
    service.in_flight.remember(thread_id, "turn-a".to_string());
    let error = service.resume_run(thread_id).await.expect_err("in flight");
    assert!(
        error.to_string().contains("in flight"),
        "unexpected resume error: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_stop_and_owned_finish_do_not_fail_persist() {
    let dir = TempDir::new().expect("tempdir");
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    let thread_id = ThreadId::from_u128(62);
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

    let stop_task = tokio::spawn({
        let service = Arc::clone(&service);
        async move { service.stop_run(thread_id).await }
    });
    let finish_task = tokio::spawn({
        let service = Arc::clone(&service);
        async move {
            service
                .finish_owned_host_turn(thread_id, "turn-a", HostCallResult::success("ok"))
                .await
        }
    });
    let stopped = stop_task.await.expect("stop join");
    let finished = finish_task.await.expect("finish join");

    match stopped {
        Ok(run) => {
            assert_ne!(run.status, WorkflowStatus::Failed);
            assert_ne!(run.error.as_deref(), Some(WORKFLOW_ERROR_HOST_RUNTIME));
        }
        Err(WorkflowServiceError::InvalidRequest(message)) => {
            assert!(
                message.contains("not active"),
                "unexpected stop error: {message}"
            );
        }
        Err(error) => panic!("stop persist raced: {error}"),
    }
    match finished {
        Ok(Some(run)) => {
            assert_ne!(run.status, WorkflowStatus::Failed);
            assert_ne!(run.error.as_deref(), Some(WORKFLOW_ERROR_HOST_RUNTIME));
        }
        Ok(None) => panic!("owned finish dropped the run"),
        Err(error) => panic!("finish persist raced: {error}"),
    }

    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert!(
        matches!(
            run.status,
            WorkflowStatus::Paused | WorkflowStatus::Complete
        ),
        "unexpected status {:?}",
        run.status
    );
    assert_ne!(run.error.as_deref(), Some(WORKFLOW_ERROR_HOST_RUNTIME));
}
