use std::sync::Arc;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::WorkflowService;
use crate::engine::SpawnBinding;
use crate::journal::HOST_ERROR_CHILD_ERRORED;
use crate::journal::HOST_ERROR_CHILD_UNAVAILABLE;
use crate::journal::HostCallResult;
use crate::journal::WORKFLOW_ERROR_HOST_RUNTIME;
use crate::run::WorkflowRun;
use crate::run::WorkflowStatus;
use crate::spawn::WorkflowSpawnAvailableFuture;
use crate::spawn::WorkflowSpawnFuture;
use crate::spawn::WorkflowSpawnHost;
use crate::spawn::WorkflowSpawnOutcome;
use crate::spawn::WorkflowSpawnRequest;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;

struct ScriptedSpawnHost {
    available: bool,
}

impl WorkflowSpawnHost for ScriptedSpawnHost {
    fn spawn_available(&self, _thread_id: ThreadId) -> WorkflowSpawnAvailableFuture<'_> {
        let available = self.available;
        Box::pin(async move { available })
    }

    fn spawn_and_wait(
        &self,
        _thread_id: ThreadId,
        _request: WorkflowSpawnRequest,
    ) -> WorkflowSpawnFuture<'_> {
        Box::pin(async { Ok(WorkflowSpawnOutcome::ChildUnavailable) })
    }
}

fn persist_spawn_run(dir: &TempDir, thread_id: ThreadId) -> WorkflowRun {
    let started = WorkflowRun::start_with_spawn(
        thread_id,
        r#"
            let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
            if r.ok { complete(); } else { ask("wrong reply"); }
        "#,
        SpawnBinding::Available,
    )
    .expect("start");
    std::fs::write(
        dir.path().join(format!("{thread_id}.json")),
        serde_json::to_vec_pretty(&started).expect("encode"),
    )
    .expect("write");
    started
}

#[tokio::test]
async fn missing_host_binds_spawn_unavailable() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    assert_eq!(
        service.spawn_binding_for(ThreadId::from_u128(90)).await,
        SpawnBinding::Unavailable
    );
}

#[tokio::test]
async fn injected_host_availability_controls_spawn_binding() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(91);
    service.set_spawn_host(Arc::new(ScriptedSpawnHost { available: true }));
    assert_eq!(
        service.spawn_binding_for(thread_id).await,
        SpawnBinding::Available
    );

    service.set_spawn_host(Arc::new(ScriptedSpawnHost { available: false }));
    assert_eq!(
        service.spawn_binding_for(thread_id).await,
        SpawnBinding::Unavailable
    );
}

#[tokio::test]
async fn start_run_uses_injected_host_for_spawn_binding() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    service.set_spawn_host(Arc::new(ScriptedSpawnHost { available: true }));
    let started = service
        .start_run(
            ThreadId::from_u128(92),
            r#"let r = agent("Say ok.", #{ "spawn": true, task_name: "review" }); complete();"#,
        )
        .await
        .expect("start");
    assert!(started.spawn_available);
    assert_eq!(started.pending_spawn_task_name.as_deref(), Some("review"));
}

#[tokio::test]
async fn completed_spawn_outcome_journals_success_including_empty_text() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(93);
    let started = persist_spawn_run(&dir, thread_id);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let wait = service.spawn_waits.remember(thread_id);
    let run = service
        .finish_spawn_wait(
            thread_id,
            &started.run_id,
            wait,
            Ok(WorkflowSpawnOutcome::Completed(String::new())),
        )
        .await
        .expect("completed");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(
        run.continuations[0].result,
        HostCallResult::success(String::new())
    );
}

#[tokio::test]
async fn child_error_outcomes_journal_stable_host_errors() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(94);
    let started = persist_spawn_run(&dir, thread_id);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let wait = service.spawn_waits.remember(thread_id);
    let run = service
        .finish_spawn_wait(
            thread_id,
            &started.run_id,
            wait,
            Ok(WorkflowSpawnOutcome::ChildErrored),
        )
        .await
        .expect("errored");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("wrong reply"));
    assert_eq!(
        run.continuations[0].result,
        HostCallResult::failure(HOST_ERROR_CHILD_ERRORED)
    );

    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(95);
    let started = persist_spawn_run(&dir, thread_id);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let wait = service.spawn_waits.remember(thread_id);
    let run = service
        .finish_spawn_wait(
            thread_id,
            &started.run_id,
            wait,
            Ok(WorkflowSpawnOutcome::ChildUnavailable),
        )
        .await
        .expect("unavailable");
    assert_eq!(
        run.continuations[0].result,
        HostCallResult::failure(HOST_ERROR_CHILD_UNAVAILABLE)
    );
}

#[tokio::test]
async fn cancelled_spawn_outcome_does_not_journal() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(96);
    let started = persist_spawn_run(&dir, thread_id);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let wait = service.spawn_waits.remember(thread_id);
    assert!(
        service
            .finish_spawn_wait(
                thread_id,
                &started.run_id,
                wait,
                Ok(WorkflowSpawnOutcome::Cancelled),
            )
            .await
            .is_none()
    );
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert!(run.continuations.is_empty());
    assert_eq!(run.pending_spawn_task_name.as_deref(), Some("review"));
}

#[tokio::test]
async fn host_runtime_error_fails_the_run() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(97);
    let started = persist_spawn_run(&dir, thread_id);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let wait = service.spawn_waits.remember(thread_id);
    assert!(
        service
            .finish_spawn_wait(
                thread_id,
                &started.run_id,
                wait,
                Err(CodexErr::InvalidRequest("spawn host failed".into())),
            )
            .await
            .is_none()
    );
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some(WORKFLOW_ERROR_HOST_RUNTIME));
}
