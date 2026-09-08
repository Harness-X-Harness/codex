use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use crate::persist::load_workflow_document;
use crate::persist::persist_workflow_document;
use crate::run::WorkflowRun;
use crate::run::WorkflowStatus;
use codex_protocol::ThreadId;

const ASK_THEN_COMPLETE: &str = r#"ask("next"); complete();"#;

#[tokio::test]
async fn forget_thread_drops_cache_and_keeps_persist() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(2361);
    let started = service
        .start_run(
            thread_id,
            r#"
                write_scratch_file("note.txt", "hello");
                ask("next");
                complete();
            "#,
        )
        .await
        .expect("start");
    assert_eq!(service.cached_run_count().await, 1);
    assert!(
        load_workflow_document(dir.path(), &thread_id.to_string())
            .expect("load")
            .is_some()
    );
    let scratch = dir
        .path()
        .join(thread_id.to_string())
        .join("scratch")
        .join("note.txt");
    assert_eq!(std::fs::read_to_string(&scratch).expect("scratch"), "hello");

    service.forget_thread(thread_id).await;
    assert_eq!(service.cached_run_count().await, 0);
    assert!(
        load_workflow_document(dir.path(), &thread_id.to_string())
            .expect("persist after forget")
            .is_some()
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).expect("scratch after forget"),
        "hello"
    );

    let reloaded = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let run = reloaded
        .get_run(thread_id)
        .await
        .expect("get")
        .expect("run");
    assert_eq!(run.status, started.status);
    assert_eq!(run.run_id, started.run_id);
}

#[tokio::test]
async fn forget_thread_cancels_a_remembered_spawn_wait() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(2362);
    service
        .start_run(thread_id, ASK_THEN_COMPLETE)
        .await
        .expect("start");
    let token = service.spawn_waits.remember(thread_id);
    service
        .in_flight
        .remember(thread_id, "turn-236".to_string());
    assert_eq!(service.spawn_wait_count(), 1);
    assert!(service.in_flight.has(thread_id));
    service.forget_thread(thread_id).await;
    assert!(*token.rx.borrow());
    assert_eq!(service.spawn_wait_count(), 0);
    assert!(!service.in_flight.has(thread_id));
    assert_eq!(service.cached_run_count().await, 0);
}

#[tokio::test]
async fn repeated_start_forget_does_not_grow_the_cache() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    for n in 0..8 {
        let thread_id = ThreadId::from_u128(2370 + n);
        service
            .start_run(thread_id, "complete();")
            .await
            .expect("start");
        service.forget_thread(thread_id).await;
        assert_eq!(service.cached_run_count().await, 0);
    }
}

#[tokio::test]
async fn forget_then_get_reloads_paused_state_from_disk() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(2363);
    service
        .start_run(thread_id, ASK_THEN_COMPLETE)
        .await
        .expect("start");
    let paused = service.stop_run(thread_id).await.expect("stop");
    assert_eq!(paused.status, WorkflowStatus::Paused);
    service.forget_thread(thread_id).await;
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.run_id, paused.run_id);
}

#[tokio::test]
async fn forget_then_get_reloads_waiting_state_from_disk() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2365);
    let queued = WorkflowRun::queue(thread_id, ASK_THEN_COMPLETE).expect("queue");
    persist_workflow_document(
        dir.path(),
        &thread_id.to_string(),
        &serde_json::to_vec_pretty(&queued).expect("serialize"),
    )
    .expect("persist");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let waiting = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(waiting.status, WorkflowStatus::Waiting);
    assert_eq!(service.cached_run_count().await, 1);
    service.forget_thread(thread_id).await;
    assert_eq!(service.cached_run_count().await, 0);
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    assert_eq!(run.run_id, waiting.run_id);
}
