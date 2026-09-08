use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use crate::journal::HostCallResult;
use crate::run::WorkflowStatus;
use codex_protocol::ThreadId;

#[tokio::test]
async fn stop_cancels_spawn_wait_and_rejects_late_child_success() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(80);
    service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start");
    let run_id = service
        .get_run(thread_id)
        .await
        .expect("get")
        .expect("run")
        .run_id;
    let token = service.spawn_waits.remember(thread_id);
    let stopped = service.stop_run(thread_id).await.expect("stop");
    assert_eq!(stopped.status, WorkflowStatus::Paused);
    assert!(*token.rx.borrow());

    let error = service
        .advance_spawn_wait(thread_id, &run_id, HostCallResult::success("ok"))
        .await
        .expect_err("late");
    assert!(
        error.to_string().contains("not active"),
        "unexpected late-success error: {error}"
    );
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert!(run.continuations.is_empty());
}

#[tokio::test]
async fn fail_cancels_spawn_wait_and_rejects_late_child_success() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(81);
    service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start");
    let run_id = service
        .get_run(thread_id)
        .await
        .expect("get")
        .expect("run")
        .run_id;
    let token = service.spawn_waits.remember(thread_id);
    service
        .mutate_run(thread_id, |run| {
            run.fail("host_runtime");
            Ok(())
        })
        .await
        .expect("fail");
    assert!(*token.rx.borrow());

    let error = service
        .advance_spawn_wait(thread_id, &run_id, HostCallResult::success("ok"))
        .await
        .expect_err("late");
    assert!(
        error.to_string().contains("not active"),
        "unexpected late-success error: {error}"
    );
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert!(run.continuations.is_empty());
}

#[tokio::test]
async fn replace_does_not_let_old_wait_drop_or_journal_into_successor() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(82);
    let first = service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start first");
    let first_wait = service.spawn_waits.remember(thread_id);
    let stopped = service.stop_run(thread_id).await.expect("stop");
    assert_eq!(stopped.status, WorkflowStatus::Paused);
    assert!(*first_wait.rx.borrow());

    let second = service
        .start_run(thread_id, r#"ask("Compile the crate."); complete();"#)
        .await
        .expect("start successor");
    assert_eq!(second.status, WorkflowStatus::Active);
    assert_ne!(second.run_id, first.run_id);
    let second_wait = service.spawn_waits.remember(thread_id);
    assert!(!*second_wait.rx.borrow());

    service.spawn_waits.forget(&first_wait);
    assert!(!*second_wait.rx.borrow());

    service
        .mutate_run(thread_id, |run| {
            run.mark_pending_yield_started();
            Ok(())
        })
        .await
        .expect("mark successor yield");

    let error = service
        .advance_spawn_wait(
            thread_id,
            &first.run_id,
            HostCallResult::success("late child"),
        )
        .await
        .expect_err("late into successor");
    assert!(
        error.to_string().contains("not active"),
        "unexpected late-success error: {error}"
    );
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.run_id, second.run_id);
    assert!(run.continuations.is_empty());
    assert!(!*second_wait.rx.borrow());
}
