//! Start, inspect, and host-resume host-owned Rhai workflow runs.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::PoisonError;
use std::sync::Weak;

use codex_core::StartIfIdleSubmission;
use codex_core::ThreadManager;
use codex_core::TurnInput;
use codex_core::TurnInputRequest;
use codex_core::TurnStartOptions;
use codex_extension_api::EngineOccupant;
use codex_extension_api::EngineSlot;
use codex_extension_api::HostIdleHold;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::engine_slot;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use tokio::sync::Mutex;

use crate::catalog::CatalogRoots;
use crate::catalog::resolve_named;
use crate::claim::OwnershipEffect;
use crate::claim::WorkflowClaim;
use crate::claim::reconcile_workflow_ownership;
use crate::engine::SpawnBinding;
use crate::engine::WorkflowSourceError;
use crate::inflight::InFlightTurns;
use crate::journal::HOST_ERROR_CHILD_ERRORED;
use crate::journal::HOST_ERROR_CHILD_UNAVAILABLE;
use crate::journal::HostCallResult;
use crate::journal::WORKFLOW_ERROR_HOST_RUNTIME;
use crate::persist::MAX_WORKFLOW_PERSIST_BYTES;
use crate::persist::PersistError;
use crate::persist::load_workflow_document;
use crate::persist::persist_workflow_document;
use crate::run::WorkflowRun;
use crate::run::WorkflowStatus;
use crate::spawn::WorkflowSpawnHost;
use crate::spawn::WorkflowSpawnOutcome;
use crate::spawn::WorkflowSpawnRequest;
use crate::spawn_waits::SpawnWait;
use crate::spawn_waits::SpawnWaits;
use crate::steering::yield_steering_item;

/// Errors from the workflow service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowServiceError {
    InvalidRequest(String),
    Internal(String),
}

impl fmt::Display for WorkflowServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for WorkflowServiceError {}

impl From<WorkflowSourceError> for WorkflowServiceError {
    fn from(error: WorkflowSourceError) -> Self {
        Self::InvalidRequest(error.to_string())
    }
}

/// Async sink invoked after a persisted run changes.
pub type WorkflowUpdateSink =
    Arc<dyn Fn(WorkflowRun) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Process-scoped workflow runs, persisted as JSON under `persist_root`.
pub struct WorkflowService {
    this: Weak<Self>,
    persist_root: PathBuf,
    project_root: PathBuf,
    runs: Mutex<HashMap<String, WorkflowRun>>,
    /// Serializes persist+remember so concurrent mutations cannot race the
    /// shared `{thread_id}.json.tmp` staging file or persist a stale snapshot.
    persist_lock: Mutex<()>,
    in_flight: InFlightTurns,
    spawn_waits: SpawnWaits,
    thread_manager: Weak<ThreadManager>,
    spawn_host: StdMutex<Option<Arc<dyn WorkflowSpawnHost>>>,
    update_sink: StdMutex<Option<WorkflowUpdateSink>>,
}

impl WorkflowService {
    pub fn new(persist_root: impl Into<PathBuf>, thread_manager: Weak<ThreadManager>) -> Self {
        Self::with_project_root(persist_root, PathBuf::new(), thread_manager)
    }

    pub fn with_project_root(
        persist_root: impl Into<PathBuf>,
        project_root: impl Into<PathBuf>,
        thread_manager: Weak<ThreadManager>,
    ) -> Self {
        Self {
            this: Weak::new(),
            persist_root: persist_root.into(),
            project_root: project_root.into(),
            runs: Mutex::new(HashMap::new()),
            persist_lock: Mutex::new(()),
            in_flight: InFlightTurns::default(),
            spawn_waits: SpawnWaits::default(),
            thread_manager,
            spawn_host: StdMutex::new(None),
            update_sink: StdMutex::new(None),
        }
    }

    /// Shared service whose stock-spawn waits can run in the background.
    pub fn shared_with_project_root(
        persist_root: impl Into<PathBuf>,
        project_root: impl Into<PathBuf>,
        thread_manager: Weak<ThreadManager>,
        spawn_host: Arc<dyn WorkflowSpawnHost>,
    ) -> Arc<Self> {
        let persist_root = persist_root.into();
        let project_root = project_root.into();
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            persist_root,
            project_root,
            runs: Mutex::new(HashMap::new()),
            persist_lock: Mutex::new(()),
            in_flight: InFlightTurns::default(),
            spawn_waits: SpawnWaits::default(),
            thread_manager,
            spawn_host: StdMutex::new(Some(spawn_host)),
            update_sink: StdMutex::new(None),
        })
    }

    pub fn set_update_sink(&self, sink: WorkflowUpdateSink) {
        *self
            .update_sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(sink);
    }

    pub fn set_spawn_host(&self, host: Arc<dyn WorkflowSpawnHost>) {
        *self
            .spawn_host
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(host);
    }

    fn spawn_host(&self) -> Option<Arc<dyn WorkflowSpawnHost>> {
        self.spawn_host
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub async fn get_run(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        self.load_cached_or_disk(&thread_id.to_string()).await
    }

    pub async fn start_run(
        &self,
        thread_id: ThreadId,
        source: &str,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let scratch_dir = self.scratch_dir_for(thread_id);
        let spawn = self.spawn_binding_for(thread_id).await;
        self.start_prepared(thread_id, |thread_id, claimed| {
            if claimed {
                WorkflowRun::start_named_with_scratch_and_spawn(
                    thread_id,
                    "workflow",
                    source,
                    serde_json::Map::new(),
                    scratch_dir.clone(),
                    spawn,
                )
            } else {
                let mut run = WorkflowRun::queue(thread_id, source)?;
                run.bind_scratch_dir(scratch_dir);
                run.bind_spawn(spawn);
                Ok(run)
            }
        })
        .await
    }

    pub async fn start_named_run(
        &self,
        thread_id: ThreadId,
        name: &str,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let args_body = serde_json::to_vec(&args).map_err(|error| {
            WorkflowServiceError::InvalidRequest(format!(
                "failed to serialize workflow args: {error}"
            ))
        })?;
        if args_body.len() > MAX_WORKFLOW_PERSIST_BYTES {
            return Err(WorkflowServiceError::InvalidRequest(format!(
                "workflow args exceed {MAX_WORKFLOW_PERSIST_BYTES} bytes"
            )));
        }
        let roots = self.catalog_roots();
        let script = resolve_named(name, &roots)
            .map_err(|error| WorkflowServiceError::InvalidRequest(error.to_string()))?;
        let scratch_dir = self.scratch_dir_for(thread_id);
        let spawn = self.spawn_binding_for(thread_id).await;
        self.start_prepared(thread_id, |thread_id, claimed| {
            if claimed {
                WorkflowRun::start_named_with_scratch_and_spawn(
                    thread_id,
                    script.name,
                    &script.source,
                    args,
                    scratch_dir.clone(),
                    spawn,
                )
            } else {
                let mut run =
                    WorkflowRun::queue_named(thread_id, script.name, &script.source, args)?;
                run.bind_scratch_dir(scratch_dir);
                run.bind_spawn(spawn);
                Ok(run)
            }
        })
        .await
    }

    async fn spawn_binding_for(&self, thread_id: ThreadId) -> SpawnBinding {
        let Some(host) = self.spawn_host() else {
            return SpawnBinding::Unavailable;
        };
        if host.spawn_available(thread_id).await {
            SpawnBinding::Available
        } else {
            SpawnBinding::Unavailable
        }
    }

    fn scratch_dir_for(&self, thread_id: ThreadId) -> PathBuf {
        self.persist_root
            .join(thread_id.to_string())
            .join("scratch")
    }

    fn catalog_roots(&self) -> CatalogRoots {
        if self.project_root.as_os_str().is_empty() {
            CatalogRoots {
                user_dir: self.persist_root.clone(),
                project_dir: self.persist_root.join("__no_project__"),
            }
        } else {
            CatalogRoots::from_persist_and_cwd(&self.persist_root, &self.project_root)
        }
    }

    async fn start_prepared(
        &self,
        thread_id: ThreadId,
        build: impl FnOnce(ThreadId, bool) -> Result<WorkflowRun, String>,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let key = thread_id.to_string();
        if self.load_cached_or_disk(&key).await?.is_some_and(|run| {
            matches!(run.status, WorkflowStatus::Active | WorkflowStatus::Waiting)
        }) {
            return Err(WorkflowServiceError::InvalidRequest(
                "a workflow is already active; /workflow stop first".to_string(),
            ));
        }
        self.spawn_waits.cancel(thread_id);
        let mut claim = self.claim_workflow(thread_id).await;
        claim.rollback_if_held();
        let claimed = claim.succeeded();
        let run = match build(thread_id, claimed) {
            Ok(run) => run,
            Err(error) => return Err(WorkflowServiceError::InvalidRequest(error)),
        };
        {
            let _persist = self.persist_lock.lock().await;
            persist_run(&self.persist_root, &run)?;
            claim.commit();
            self.remember(key, run.clone()).await;
        }
        self.after_run_changed(&run).await;
        self.kick_if_active(&run).await;
        Ok(run)
    }

    pub async fn advance_run(
        &self,
        thread_id: ThreadId,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let Some(run) = self.get_run(thread_id).await? else {
            return Err(WorkflowServiceError::InvalidRequest(
                "no workflow is set for this thread".to_string(),
            ));
        };
        match run.status {
            WorkflowStatus::Active => self.kick_if_active(&run).await,
            WorkflowStatus::Waiting => self
                .continue_if_idle(thread_id)
                .await
                .map_err(WorkflowServiceError::InvalidRequest)?,
            WorkflowStatus::Paused => {
                return Err(WorkflowServiceError::InvalidRequest(
                    "workflow is paused; use thread/workflow/resume".to_string(),
                ));
            }
            WorkflowStatus::Complete | WorkflowStatus::Failed => {}
        }
        self.get_run(thread_id).await?.ok_or_else(|| {
            WorkflowServiceError::InvalidRequest("no workflow is set for this thread".to_string())
        })
    }

    /// Marks the live run as having started its next same-Thread host turn.
    ///
    /// Production scheduling does this immediately before submitting the turn.
    /// In-process tests use the same path so they do not simulate a restart.
    pub async fn mark_pending_yield_started(
        &self,
        thread_id: ThreadId,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        self.mutate_run(thread_id, |run| {
            run.mark_pending_yield_started();
            Ok(())
        })
        .await
    }

    pub async fn finish_yield_turn_with_result(
        &self,
        thread_id: ThreadId,
        result: HostCallResult,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        self.apply_owned_host_turn(thread_id, /*turn_id*/ None, result)
            .await
    }

    pub(crate) async fn finish_owned_host_turn(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        result: HostCallResult,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        self.apply_owned_host_turn(thread_id, Some(turn_id), result)
            .await
    }

    async fn apply_owned_host_turn(
        &self,
        thread_id: ThreadId,
        turn_id: Option<&str>,
        result: HostCallResult,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        let Some(existing) = self.get_run(thread_id).await? else {
            return Ok(None);
        };
        if !existing.pending_yield_started
            || existing.pending_spawn_task_name.is_some()
            || !matches!(
                existing.status,
                WorkflowStatus::Active | WorkflowStatus::Paused
            )
            || !self.in_flight.owns(thread_id, turn_id)
        {
            return Ok(Some(existing));
        }
        let run = self
            .mutate_run(thread_id, move |run| run.apply_owned_host_result(result))
            .await?;
        self.in_flight.forget(thread_id);
        if run.occupies_idle() {
            self.kick_if_active(&run).await;
        } else {
            // Occupancy is already released. Stock idle after a natural
            // terminal starts a waiting Goal HOW; notify again in case the
            // first idle raced the owned-result apply.
            self.kick_waiting_goal(run.thread_id).await;
        }
        Ok(Some(run))
    }

    pub async fn stop_run(&self, thread_id: ThreadId) -> Result<WorkflowRun, WorkflowServiceError> {
        // Quarantine a started same-Thread yield. Do not Interrupt: stock
        // abort applies the owned terminal before mailbox teardown and never
        // emits a later idle, so a waiting Goal HOW would never start.
        self.mutate_run(thread_id, WorkflowRun::stop).await
    }

    /// Drop process-local runtime for a stopped Thread. Persist and scratch stay.
    pub async fn forget_thread(&self, thread_id: ThreadId) {
        self.spawn_waits.drop_thread(thread_id);
        self.in_flight.forget(thread_id);
        self.runs.lock().await.remove(&thread_id.to_string());
        let Some(thread) = self.live_thread(thread_id).await else {
            return;
        };
        engine_slot(thread.thread_extension_data()).release(EngineOccupant::Workflow);
        thread.thread_extension_data().remove::<HostIdleHold>();
    }

    #[cfg(test)]
    pub(crate) async fn cached_run_count(&self) -> usize {
        self.runs.lock().await.len()
    }

    #[cfg(test)]
    pub(crate) fn spawn_wait_count(&self) -> usize {
        self.spawn_waits.len()
    }

    pub async fn resume_run(
        &self,
        thread_id: ThreadId,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        if self.in_flight.has(thread_id)
            || self
                .get_run(thread_id)
                .await?
                .is_some_and(|run| run.pending_yield_started)
        {
            return Err(WorkflowServiceError::InvalidRequest(
                "workflow host turn is still in flight".to_string(),
            ));
        }
        let claim = self.claim_workflow(thread_id).await;
        let run = if claim.succeeded() {
            self.mutate_run(thread_id, WorkflowRun::resume).await
        } else {
            self.mutate_run(thread_id, WorkflowRun::park).await
        };
        let run = run?;
        claim.commit();
        self.kick_if_active(&run).await;
        Ok(run)
    }

    pub async fn restore_occupancy(&self, thread_id: ThreadId) -> Result<(), String> {
        let Some(run) = self
            .get_run(thread_id)
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        let Some(slot) = self.workflow_slot(thread_id).await else {
            self.refresh_idle_hold(&run).await;
            return Ok(());
        };
        let mut snapshot = run;
        match reconcile_workflow_ownership(&slot, &mut snapshot) {
            OwnershipEffect::Parked => {
                self.mutate_run(thread_id, WorkflowRun::yield_occupancy)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OwnershipEffect::Held | OwnershipEffect::Released { .. } => {
                self.refresh_idle_hold(&snapshot).await;
            }
        }
        Ok(())
    }

    pub async fn continue_if_idle(&self, thread_id: ThreadId) -> Result<(), String> {
        let Some(run) = self
            .get_run(thread_id)
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        let run = if run.status == WorkflowStatus::Waiting {
            let claim = self.claim_workflow(thread_id).await;
            if !claim.succeeded() {
                return Ok(());
            }
            let run = match self
                .mutate_run(thread_id, |run| run.activate().map(|_| ()))
                .await
            {
                Ok(run) => run,
                Err(err) => return Err(err.to_string()),
            };
            claim.commit();
            run
        } else {
            run
        };
        if run.status != WorkflowStatus::Active {
            return Ok(());
        }
        if !self.try_claim_workflow(thread_id).await {
            return Ok(());
        }
        if run.pending_yield_started || self.in_flight.has(thread_id) {
            return Ok(());
        }
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            tracing::debug!("skipping workflow continuation because thread manager is unavailable");
            return Ok(());
        };
        let Ok(thread) = thread_manager.get_thread(thread_id).await else {
            tracing::debug!("skipping workflow continuation because live thread is unavailable");
            return Ok(());
        };
        let mut run = run;
        loop {
            if run.status != WorkflowStatus::Active
                || run.pending_yield_started
                || self.in_flight.has(thread_id)
            {
                return Ok(());
            }
            let Some(instruction) = run.pending_instruction.clone() else {
                return Ok(());
            };
            if let Some(task_name) = run.pending_spawn_task_name.clone() {
                let run_id = run.run_id.clone();
                let wait = self.spawn_waits.remember(thread_id);
                if let Err(err) = self
                    .mutate_run(thread_id, |run| {
                        run.mark_pending_yield_started();
                        Ok(())
                    })
                    .await
                {
                    self.spawn_waits.forget(&wait);
                    tracing::debug!("failed to mark workflow spawn started for {thread_id}: {err}");
                    return Ok(());
                }
                let Some(host) = self.spawn_host() else {
                    let _ = self
                        .finish_spawn_wait(
                            thread_id,
                            &run_id,
                            wait,
                            Err(CodexErr::InvalidRequest(
                                "workflow spawn host is unavailable".to_string(),
                            )),
                        )
                        .await;
                    return Ok(());
                };
                let request = WorkflowSpawnRequest {
                    message: instruction.clone(),
                    task_name,
                    cancel: wait.rx.clone(),
                };
                if let Some(service) = self.this.upgrade() {
                    tokio::spawn(async move {
                        let outcome = host.spawn_and_wait(thread_id, request).await;
                        if let Some(updated) = service
                            .finish_spawn_wait(thread_id, &run_id, wait, outcome)
                            .await
                        {
                            // Re-enter through the Send-safe idle notify. The
                            // parent Thread has no parent turn during spawn, so
                            // it is idle; `continue_if_idle` is not Send.
                            if updated.status == WorkflowStatus::Active
                                && updated.pending_instruction.is_some()
                            {
                                service.kick_waiting_goal(updated.thread_id).await;
                            }
                        }
                    });
                    return Ok(());
                }
                let outcome = host.spawn_and_wait(thread_id, request).await;
                if let Some(updated) = self
                    .finish_spawn_wait(thread_id, &run_id, wait, outcome)
                    .await
                {
                    run = updated;
                    continue;
                }
                return Ok(());
            }
            let start_options = thread
                .thread_extension_data()
                .get::<TurnStartOptions>()
                .map(|options| options.as_ref().clone())
                .unwrap_or_default();
            let item = yield_steering_item(&run, &instruction);
            match thread
                .start_turn_if_idle(
                    TurnInputRequest::new(TurnInput::ResponseItem(item)).on_start(
                        TurnStartOptions {
                            turn_trigger: Some("workflow".to_string()),
                            ..start_options
                        },
                    ),
                )
                .await
            {
                Ok(StartIfIdleSubmission::Started { turn_id }) => {
                    self.in_flight.remember(thread_id, turn_id);
                    if let Err(err) = self
                        .mutate_run(thread_id, |run| {
                            run.mark_pending_yield_started();
                            Ok(())
                        })
                        .await
                    {
                        tracing::debug!(
                            "failed to mark workflow yield started for {thread_id}: {err}"
                        );
                    }
                }
                Ok(StartIfIdleSubmission::NotSubmitted { reason }) => {
                    tracing::debug!(
                        ?reason,
                        "skipping workflow continuation because automatic idle work was rejected"
                    );
                }
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "skipping workflow continuation because turn input submission failed"
                    );
                }
            }
            return Ok(());
        }
    }

    async fn finish_spawn_wait(
        &self,
        thread_id: ThreadId,
        run_id: &str,
        wait: SpawnWait,
        outcome: Result<WorkflowSpawnOutcome, CodexErr>,
    ) -> Option<WorkflowRun> {
        self.spawn_waits.forget(&wait);
        match outcome {
            Ok(WorkflowSpawnOutcome::Cancelled) => None,
            Ok(WorkflowSpawnOutcome::Completed(reply)) => self
                .advance_spawn_wait(thread_id, run_id, HostCallResult::success(reply))
                .await
                .map_err(|err| {
                    tracing::debug!(
                        "failed to host-resume workflow after spawn for {thread_id}: {err}"
                    );
                })
                .ok(),
            Ok(WorkflowSpawnOutcome::ChildErrored) => self
                .advance_spawn_wait(
                    thread_id,
                    run_id,
                    HostCallResult::failure(HOST_ERROR_CHILD_ERRORED),
                )
                .await
                .map_err(|err| {
                    tracing::debug!(
                        "failed to host-resume workflow after spawn failure for {thread_id}: {err}"
                    );
                })
                .ok(),
            Ok(WorkflowSpawnOutcome::ChildUnavailable) => self
                .advance_spawn_wait(
                    thread_id,
                    run_id,
                    HostCallResult::failure(HOST_ERROR_CHILD_UNAVAILABLE),
                )
                .await
                .map_err(|err| {
                    tracing::debug!(
                        "failed to host-resume workflow after spawn failure for {thread_id}: {err}"
                    );
                })
                .ok(),
            Err(error) => {
                tracing::debug!(
                    %error,
                    "workflow spawn failed with an unrecoverable host/runtime error"
                );
                if let Err(err) = self
                    .mutate_run(thread_id, |run| {
                        run.fail(WORKFLOW_ERROR_HOST_RUNTIME);
                        Ok(())
                    })
                    .await
                {
                    tracing::debug!(
                        "failed to mark workflow spawn runtime failure for {thread_id}: {err}"
                    );
                }
                None
            }
        }
    }

    async fn advance_spawn_wait(
        &self,
        thread_id: ThreadId,
        run_id: &str,
        result: HostCallResult,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let run_id = run_id.to_string();
        self.mutate_run(thread_id, move |run| {
            if run.run_id != run_id {
                return Err("workflow is not active".to_string());
            }
            run.advance_with_outcome(result).map(|_| ())
        })
        .await
    }

    async fn mutate_run(
        &self,
        thread_id: ThreadId,
        mutate: impl FnOnce(&mut WorkflowRun) -> Result<(), String>,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let key = thread_id.to_string();
        let (run, persist_error) = {
            let _persist = self.persist_lock.lock().await;
            let (loaded, _) = self.load_cached_or_disk_parts(&key).await?;
            let mut run = loaded.ok_or_else(|| {
                WorkflowServiceError::InvalidRequest(
                    "no workflow is set for this thread".to_string(),
                )
            })?;
            mutate(&mut run).map_err(WorkflowServiceError::InvalidRequest)?;
            let persist_error = persist_run(&self.persist_root, &run).err();
            if persist_error.is_some() {
                run.fail(WORKFLOW_ERROR_HOST_RUNTIME);
            }
            self.remember(key, run.clone()).await;
            (run, persist_error)
        };
        self.after_run_changed(&run).await;
        match persist_error {
            Some(error) => Err(error),
            None => Ok(run),
        }
    }

    async fn load_cached_or_disk(
        &self,
        key: &str,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        let (run, notify_failed) = self.load_cached_or_disk_parts(key).await?;
        if notify_failed {
            if let Some(run) = &run {
                self.after_run_changed(run).await;
            }
        }
        Ok(run)
    }

    async fn load_cached_or_disk_parts(
        &self,
        key: &str,
    ) -> Result<(Option<WorkflowRun>, bool), WorkflowServiceError> {
        if let Some(run) = self.runs.lock().await.get(key).cloned() {
            return Ok((Some(run), false));
        }
        let Some((mut run, notify_failed)) = load_run(&self.persist_root, key)? else {
            return Ok((None, false));
        };
        run.bind_scratch_dir(self.scratch_dir_for(run.thread_id));
        self.remember(key.to_string(), run.clone()).await;
        Ok((Some(run), notify_failed))
    }

    async fn remember(&self, key: String, run: WorkflowRun) {
        self.runs.lock().await.insert(key, run);
    }

    async fn after_run_changed(&self, run: &WorkflowRun) {
        if run.status != WorkflowStatus::Active {
            self.spawn_waits.cancel(run.thread_id);
        }
        let released = self.refresh_idle_hold(run).await;
        let sink = self
            .update_sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(sink) = sink {
            sink(run.clone()).await;
        }
        if released {
            self.kick_waiting_goal(run.thread_id).await;
        }
    }

    async fn refresh_idle_hold(&self, run: &WorkflowRun) -> bool {
        let Some(thread) = self.live_thread(run.thread_id).await else {
            return false;
        };
        let slot = engine_slot(thread.thread_extension_data());
        if run.occupies_idle() {
            if slot.try_claim(EngineOccupant::Workflow) {
                thread.thread_extension_data().insert(HostIdleHold);
            } else {
                thread.thread_extension_data().remove::<HostIdleHold>();
            }
            false
        } else {
            let released = slot.release(EngineOccupant::Workflow);
            thread.thread_extension_data().remove::<HostIdleHold>();
            released
        }
    }

    async fn workflow_slot(&self, thread_id: ThreadId) -> Option<Arc<EngineSlot>> {
        let thread = self.live_thread(thread_id).await?;
        Some(engine_slot(thread.thread_extension_data()))
    }

    async fn claim_workflow(&self, thread_id: ThreadId) -> WorkflowClaim {
        match self.workflow_slot(thread_id).await {
            Some(slot) => WorkflowClaim::acquire(slot),
            None => WorkflowClaim::vacuous(),
        }
    }

    async fn kick_waiting_goal(&self, thread_id: ThreadId) {
        let Some(thread) = self.live_thread(thread_id).await else {
            return;
        };
        thread
            .emit_thread_idle_lifecycle_if_idle(ThreadIdleCause::Completed)
            .await;
    }

    async fn try_claim_workflow(&self, thread_id: ThreadId) -> bool {
        let Some(thread) = self.live_thread(thread_id).await else {
            return true;
        };
        engine_slot(thread.thread_extension_data()).try_claim(EngineOccupant::Workflow)
    }

    async fn live_thread(&self, thread_id: ThreadId) -> Option<Arc<codex_core::CodexThread>> {
        let thread_manager = self.thread_manager.upgrade()?;
        thread_manager.get_thread(thread_id).await.ok()
    }

    async fn kick_if_active(&self, run: &WorkflowRun) {
        if run.status != WorkflowStatus::Active || run.pending_instruction.is_none() {
            return;
        }
        if let Err(err) = self.continue_if_idle(run.thread_id).await {
            tracing::debug!("workflow idle kick failed for {}: {err}", run.thread_id);
        }
    }
}

fn persist_run(persist_root: &Path, run: &WorkflowRun) -> Result<(), WorkflowServiceError> {
    let body = serde_json::to_vec_pretty(run).map_err(|error| {
        WorkflowServiceError::Internal(format!("failed to serialize workflow: {error}"))
    })?;
    persist_workflow_document(persist_root, &run.thread_id.to_string(), &body).map_err(|error| {
        match error {
            PersistError::TooLarge { .. } => {
                WorkflowServiceError::InvalidRequest(error.to_string())
            }
            PersistError::UnsafePath(_) | PersistError::Io(_) => {
                WorkflowServiceError::Internal(error.to_string())
            }
        }
    })
}

fn load_run(
    persist_root: &Path,
    thread_id: &str,
) -> Result<Option<(WorkflowRun, bool)>, WorkflowServiceError> {
    let bytes = match load_workflow_document(persist_root, thread_id) {
        Ok(bytes) => bytes,
        Err(error) => return Err(WorkflowServiceError::Internal(error.to_string())),
    };
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let mut run: WorkflowRun = match serde_json::from_slice(&bytes) {
        Ok(run) => run,
        Err(_) => {
            let parsed_id = ThreadId::from_string(thread_id).map_err(|err| {
                WorkflowServiceError::Internal(format!("invalid workflow thread id: {err}"))
            })?;
            return Ok(Some((WorkflowRun::unreadable(parsed_id), true)));
        }
    };
    let prior_status = run.status;
    run.prepare_restored()
        .map_err(WorkflowServiceError::InvalidRequest)?;
    let became_failed =
        run.status == WorkflowStatus::Failed && prior_status != WorkflowStatus::Failed;
    if became_failed {
        persist_run(persist_root, &run)?;
    }
    Ok(Some((run, became_failed)))
}

/// Shared handle used by App Server and the extension install path.
pub type SharedWorkflowService = Arc<WorkflowService>;

#[cfg(test)]
#[path = "service_owned_turn_tests.rs"]
mod owned_turn_tests;

#[cfg(test)]
#[path = "service_spawn_wait_tests.rs"]
mod spawn_wait_tests;

#[cfg(test)]
#[path = "service_restore_tests.rs"]
mod restore_tests;

#[cfg(test)]
#[path = "service_forget_tests.rs"]
mod forget_tests;

#[cfg(test)]
#[path = "spawn_host_tests.rs"]
mod spawn_host_tests;
