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
use codex_core::content_items_to_text;
use codex_extension_api::EngineOccupant;
use codex_extension_api::HostIdleHold;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::engine_slot;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_rollout::RolloutItem;
use tokio::sync::Mutex;

use crate::catalog::CatalogRoots;
use crate::catalog::resolve_named;
use crate::engine::WorkflowSourceError;
use crate::engine::truncate_workflow_reply;
use crate::run::WorkflowRun;
use crate::run::WorkflowStatus;
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
    persist_root: PathBuf,
    project_root: PathBuf,
    runs: Mutex<HashMap<String, WorkflowRun>>,
    thread_manager: Weak<ThreadManager>,
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
            persist_root: persist_root.into(),
            project_root: project_root.into(),
            runs: Mutex::new(HashMap::new()),
            thread_manager,
            update_sink: StdMutex::new(None),
        }
    }

    pub fn set_update_sink(&self, sink: WorkflowUpdateSink) {
        *self
            .update_sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(sink);
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
        self.start_prepared(thread_id, |thread_id, claimed| {
            if claimed {
                WorkflowRun::start_with_scratch(thread_id, source, scratch_dir.clone())
            } else {
                let mut run = WorkflowRun::queue(thread_id, source)?;
                run.bind_scratch_dir(scratch_dir);
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
        let roots = self.catalog_roots();
        let script = resolve_named(name, &roots)
            .map_err(|error| WorkflowServiceError::InvalidRequest(error.to_string()))?;
        let scratch_dir = self.scratch_dir_for(thread_id);
        self.start_prepared(thread_id, |thread_id, claimed| {
            if claimed {
                WorkflowRun::start_named_with_scratch(
                    thread_id,
                    script.name,
                    &script.source,
                    args,
                    scratch_dir.clone(),
                )
            } else {
                let mut run =
                    WorkflowRun::queue_named(thread_id, script.name, &script.source, args)?;
                run.bind_scratch_dir(scratch_dir);
                Ok(run)
            }
        })
        .await
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
        let claimed = self.try_claim_workflow(thread_id).await;
        let run = build(thread_id, claimed).map_err(WorkflowServiceError::InvalidRequest)?;
        persist_run(&self.persist_root, &run).await?;
        self.remember(key, run.clone()).await;
        self.after_run_changed(&run).await;
        self.kick_if_active(&run).await;
        Ok(run)
    }

    pub async fn advance_run(
        &self,
        thread_id: ThreadId,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let reply = match self.get_run(thread_id).await? {
            Some(run) if run.pending_yield_started => self.latest_assistant_reply(thread_id).await,
            _ => String::new(),
        };
        let run = self
            .mutate_run(thread_id, move |run| {
                run.advance_with_reply(reply).map(|_| ())
            })
            .await?;
        self.kick_if_active(&run).await;
        Ok(run)
    }

    pub async fn finish_yield_turn(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        let Some(existing) = self.get_run(thread_id).await? else {
            return Ok(None);
        };
        if existing.status != WorkflowStatus::Active || !existing.pending_yield_started {
            return Ok(Some(existing));
        }
        let reply = self.latest_assistant_reply(thread_id).await;
        let run = self
            .mutate_run(thread_id, move |run| {
                if run.status != WorkflowStatus::Active || !run.pending_yield_started {
                    return Ok(());
                }
                run.advance_with_reply(reply).map(|_| ())
            })
            .await?;
        self.kick_if_active(&run).await;
        Ok(Some(run))
    }

    pub async fn stop_run(&self, thread_id: ThreadId) -> Result<WorkflowRun, WorkflowServiceError> {
        self.mutate_run(thread_id, WorkflowRun::stop).await
    }

    pub async fn resume_run(
        &self,
        thread_id: ThreadId,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let run = if self.try_claim_workflow(thread_id).await {
            self.mutate_run(thread_id, WorkflowRun::resume).await?
        } else {
            self.mutate_run(thread_id, WorkflowRun::park).await?
        };
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
        self.refresh_idle_hold(&run).await;
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
            if !self.try_claim_workflow(thread_id).await {
                return Ok(());
            }
            self.mutate_run(thread_id, |run| run.activate().map(|_| ()))
                .await
                .map_err(|err| err.to_string())?
        } else {
            run
        };
        if run.status != WorkflowStatus::Active {
            return Ok(());
        }
        if !self.try_claim_workflow(thread_id).await {
            return Ok(());
        }
        if run.pending_yield_started {
            return Ok(());
        }
        let Some(instruction) = run.pending_instruction.as_deref() else {
            return Ok(());
        };
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            tracing::debug!("skipping workflow continuation because thread manager is unavailable");
            return Ok(());
        };
        let Ok(thread) = thread_manager.get_thread(thread_id).await else {
            tracing::debug!("skipping workflow continuation because live thread is unavailable");
            return Ok(());
        };
        let start_options = thread
            .thread_extension_data()
            .get::<TurnStartOptions>()
            .map(|options| options.as_ref().clone())
            .unwrap_or_default();
        let item = yield_steering_item(&run, instruction);
        match thread
            .start_turn_if_idle(
                TurnInputRequest::new(TurnInput::ResponseItem(item)).on_start(TurnStartOptions {
                    turn_trigger: Some("workflow".to_string()),
                    ..start_options
                }),
            )
            .await
        {
            Ok(StartIfIdleSubmission::Started { .. }) => {
                if let Err(err) = self
                    .mutate_run(thread_id, |run| {
                        run.mark_pending_yield_started();
                        Ok(())
                    })
                    .await
                {
                    tracing::debug!("failed to mark workflow yield started for {thread_id}: {err}");
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
        Ok(())
    }

    async fn mutate_run(
        &self,
        thread_id: ThreadId,
        mutate: impl FnOnce(&mut WorkflowRun) -> Result<(), String>,
    ) -> Result<WorkflowRun, WorkflowServiceError> {
        let key = thread_id.to_string();
        let mut run = self.load_cached_or_disk(&key).await?.ok_or_else(|| {
            WorkflowServiceError::InvalidRequest("no workflow is set for this thread".to_string())
        })?;
        mutate(&mut run).map_err(WorkflowServiceError::InvalidRequest)?;
        persist_run(&self.persist_root, &run).await?;
        self.remember(key, run.clone()).await;
        self.after_run_changed(&run).await;
        Ok(run)
    }

    async fn load_cached_or_disk(
        &self,
        key: &str,
    ) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
        if let Some(run) = self.runs.lock().await.get(key).cloned() {
            return Ok(Some(run));
        }
        let Some(mut run) = load_run(&self.persist_root, key).await? else {
            return Ok(None);
        };
        run.bind_scratch_dir(self.scratch_dir_for(run.thread_id));
        self.remember(key.to_string(), run.clone()).await;
        Ok(Some(run))
    }

    async fn remember(&self, key: String, run: WorkflowRun) {
        self.runs.lock().await.insert(key, run);
    }

    async fn after_run_changed(&self, run: &WorkflowRun) {
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
            let _ = slot.try_claim(EngineOccupant::Workflow);
            thread.thread_extension_data().insert(HostIdleHold);
            false
        } else {
            let released = slot.release(EngineOccupant::Workflow);
            thread.thread_extension_data().remove::<HostIdleHold>();
            released
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

    async fn latest_assistant_reply(&self, thread_id: ThreadId) -> String {
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            return String::new();
        };
        let Ok(thread) = thread_manager.get_thread(thread_id).await else {
            return String::new();
        };
        let Ok(items) = thread
            .load_latest_model_context_items(/*include_archived*/ false)
            .await
        else {
            return String::new();
        };
        for item in items.iter().rev() {
            let RolloutItem::ResponseItem(envelope) = item else {
                continue;
            };
            let ResponseItem::Message { role, content, .. } = &envelope.item else {
                continue;
            };
            if role != "assistant" {
                continue;
            }
            if let Some(text) = content_items_to_text(content) {
                return truncate_workflow_reply(&text);
            }
        }
        String::new()
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

async fn persist_run(persist_root: &Path, run: &WorkflowRun) -> Result<(), WorkflowServiceError> {
    tokio::fs::create_dir_all(persist_root)
        .await
        .map_err(|err| {
            WorkflowServiceError::Internal(format!("failed to create workflow dir: {err}"))
        })?;
    let path = persist_root.join(format!("{}.json", run.thread_id));
    let body = serde_json::to_vec_pretty(run).map_err(|err| {
        WorkflowServiceError::Internal(format!("failed to serialize workflow: {err}"))
    })?;
    tokio::fs::write(&path, body)
        .await
        .map_err(|err| WorkflowServiceError::Internal(format!("failed to write workflow: {err}")))
}

async fn load_run(
    persist_root: &Path,
    thread_id: &str,
) -> Result<Option<WorkflowRun>, WorkflowServiceError> {
    let path = persist_root.join(format!("{thread_id}.json"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mut run: WorkflowRun = serde_json::from_slice(&bytes).map_err(|err| {
                WorkflowServiceError::Internal(format!("failed to parse workflow: {err}"))
            })?;
            run.normalize_served_replies();
            Ok(Some(run))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(WorkflowServiceError::Internal(format!(
            "failed to read workflow: {err}"
        ))),
    }
}

/// Shared handle used by App Server and the extension install path.
pub type SharedWorkflowService = Arc<WorkflowService>;
