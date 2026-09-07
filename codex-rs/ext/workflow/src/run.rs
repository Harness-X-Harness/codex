//! Host-owned workflow run state.

use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::catalog::json_args_to_map;
use crate::engine::SpawnBinding;
use crate::engine::WorkflowEval;
use crate::engine::WorkflowSourceError;
use crate::engine::eval_source_with_env;
use crate::engine::eval_source_with_scratch_and_spawn;
use crate::engine::eval_source_with_spawn;
use crate::engine::truncate_workflow_reply;
use crate::engine::validate_source;
use crate::journal::ContinuationKind;
use crate::journal::ContinuationRecord;
use crate::journal::HOST_ERROR_TURN_CANCELLED;
use crate::journal::HOST_ERROR_TURN_ERRORED;
use crate::journal::HostCallResult;
use crate::journal::REPLAY_DIVERGENCE;
use crate::journal::WORKFLOW_ERROR_HOST_RUNTIME;
use crate::journal::WORKFLOW_ERROR_LEGACY_RESUME;
use crate::journal::WORKFLOW_ERROR_REPLAY_DIVERGED;
use crate::journal::WORKFLOW_ERROR_UNSAFE_JOURNAL;
use crate::journal::WORKFLOW_PERSIST_VERSION;
use crate::journal::bounded;
use crate::journal::result_bearing_count;

/// Lifecycle of one thread's workflow run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStatus {
    Active,
    Paused,
    Complete,
    Waiting,
    Failed,
}

/// Persisted run for one thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub thread_id: ThreadId,
    pub run_id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub source: String,
    pub served_asks: u32,
    #[serde(default)]
    pub served_replies: Vec<String>,
    #[serde(default)]
    pub served_pauses: u32,
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub continuations: Vec<ContinuationRecord>,
    #[serde(default)]
    pub pending_kind: Option<ContinuationKind>,
    #[serde(default)]
    pub pending_request_digest: Option<String>,
    #[serde(default)]
    pub args: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub spawn_available: bool,
    #[serde(default)]
    pub pending_spawn_task_name: Option<String>,
    pub pending_instruction: Option<String>,
    /// True after the host started a model turn for the current yield.
    #[serde(default)]
    pub pending_yield_started: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip)]
    scratch_dir: Option<PathBuf>,
}

/// Result of a host resume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowAdvance {
    Yielded,
    Completed,
    Paused,
    Failed,
}

impl WorkflowRun {
    pub fn start(thread_id: ThreadId, source: &str) -> Result<Self, String> {
        Self::start_named(thread_id, "workflow", source, serde_json::Map::new())
    }

    pub fn start_with_spawn(
        thread_id: ThreadId,
        source: &str,
        spawn: SpawnBinding,
    ) -> Result<Self, String> {
        let now = unix_seconds();
        let mut run = Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: "workflow".to_string(),
            status: WorkflowStatus::Active,
            source: source.trim().to_string(),
            served_asks: 0,
            served_replies: Vec::new(),
            served_pauses: 0,
            format_version: WORKFLOW_PERSIST_VERSION,
            continuations: Vec::new(),
            pending_kind: None,
            pending_request_digest: None,
            args: serde_json::Map::new(),
            phase: None,
            log: None,
            result: serde_json::Value::Null,
            error: None,
            spawn_available: matches!(spawn, SpawnBinding::Available),
            pending_spawn_task_name: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
            scratch_dir: None,
        };
        let outcome = run.eval_current().map_err(|error| error.to_string())?;
        run.apply_outcome(outcome)?;
        Ok(run)
    }

    pub fn start_named(
        thread_id: ThreadId,
        name: impl Into<String>,
        source: &str,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, String> {
        let now = unix_seconds();
        let mut run = Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: name.into(),
            status: WorkflowStatus::Active,
            source: source.trim().to_string(),
            served_asks: 0,
            served_replies: Vec::new(),
            served_pauses: 0,
            format_version: WORKFLOW_PERSIST_VERSION,
            continuations: Vec::new(),
            pending_kind: None,
            pending_request_digest: None,
            args,
            phase: None,
            log: None,
            result: serde_json::Value::Null,
            error: None,
            spawn_available: false,
            pending_spawn_task_name: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
            scratch_dir: None,
        };
        let outcome = run.eval_current().map_err(|error| error.to_string())?;
        run.apply_outcome(outcome)?;
        Ok(run)
    }

    /// Persist a run that cannot occupy the engine yet.
    pub fn queue(thread_id: ThreadId, source: &str) -> Result<Self, String> {
        Self::queue_named(thread_id, "workflow", source, serde_json::Map::new())
    }

    pub fn queue_named(
        thread_id: ThreadId,
        name: impl Into<String>,
        source: &str,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, String> {
        validate_source(source).map_err(|error| error.to_string())?;
        let now = unix_seconds();
        Ok(Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: name.into(),
            status: WorkflowStatus::Waiting,
            source: source.trim().to_string(),
            served_asks: 0,
            served_replies: Vec::new(),
            served_pauses: 0,
            format_version: WORKFLOW_PERSIST_VERSION,
            continuations: Vec::new(),
            pending_kind: None,
            pending_request_digest: None,
            args,
            phase: None,
            log: None,
            result: serde_json::Value::Null,
            error: None,
            spawn_available: false,
            pending_spawn_task_name: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
            scratch_dir: None,
        })
    }

    /// Start a run that can read and write thread-local scratch files.
    pub fn start_with_scratch(
        thread_id: ThreadId,
        source: &str,
        scratch_dir: PathBuf,
    ) -> Result<Self, String> {
        Self::start_named_with_scratch(
            thread_id,
            "workflow",
            source,
            serde_json::Map::new(),
            scratch_dir,
        )
    }

    pub(crate) fn start_named_with_scratch(
        thread_id: ThreadId,
        name: impl Into<String>,
        source: &str,
        args: serde_json::Map<String, serde_json::Value>,
        scratch_dir: PathBuf,
    ) -> Result<Self, String> {
        Self::start_named_with_scratch_and_spawn(
            thread_id,
            name,
            source,
            args,
            scratch_dir,
            SpawnBinding::Unavailable,
        )
    }

    pub(crate) fn start_named_with_scratch_and_spawn(
        thread_id: ThreadId,
        name: impl Into<String>,
        source: &str,
        args: serde_json::Map<String, serde_json::Value>,
        scratch_dir: PathBuf,
        spawn: SpawnBinding,
    ) -> Result<Self, String> {
        let now = unix_seconds();
        let mut run = Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: name.into(),
            status: WorkflowStatus::Active,
            source: source.trim().to_string(),
            served_asks: 0,
            served_replies: Vec::new(),
            served_pauses: 0,
            format_version: WORKFLOW_PERSIST_VERSION,
            continuations: Vec::new(),
            pending_kind: None,
            pending_request_digest: None,
            args,
            phase: None,
            log: None,
            result: serde_json::Value::Null,
            error: None,
            spawn_available: matches!(spawn, SpawnBinding::Available),
            pending_spawn_task_name: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
            scratch_dir: Some(scratch_dir),
        };
        let outcome = run.eval_current().map_err(|error| error.to_string())?;
        run.apply_outcome(outcome)?;
        Ok(run)
    }

    pub(crate) fn bind_scratch_dir(&mut self, scratch_dir: PathBuf) {
        self.scratch_dir = Some(scratch_dir);
    }

    pub(crate) fn bind_spawn(&mut self, spawn: SpawnBinding) {
        self.spawn_available = matches!(spawn, SpawnBinding::Available);
    }

    pub fn activate(&mut self) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Waiting {
            return Err("workflow is not waiting".to_string());
        }
        if self.pending_instruction.is_some() {
            self.status = WorkflowStatus::Active;
            self.updated_at = unix_seconds();
            return Ok(WorkflowAdvance::Yielded);
        }
        match self.eval_current() {
            Ok(outcome) => self.apply_outcome(outcome),
            Err(error) => {
                self.fail(unrecoverable_error_code(&error));
                Ok(WorkflowAdvance::Failed)
            }
        }
    }

    pub fn park(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Paused {
            return Err("workflow is not paused".to_string());
        }
        self.status = WorkflowStatus::Waiting;
        self.updated_at = unix_seconds();
        Ok(())
    }

    /// Demote an `active` occupant to waiting FIFO when the engine slot is
    /// not actually owned.
    pub fn yield_occupancy(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        self.status = WorkflowStatus::Waiting;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn advance(&mut self) -> Result<WorkflowAdvance, String> {
        self.advance_with_reply(String::new())
    }

    pub fn advance_with_reply(&mut self, reply: String) -> Result<WorkflowAdvance, String> {
        self.advance_with_outcome(HostCallResult::success(truncate_workflow_reply(&reply)))
    }

    pub fn advance_with_outcome(
        &mut self,
        result: HostCallResult,
    ) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        self.consume_pending_yield(result)
    }

    /// Apply the exact owned same-Thread host result after stop or completion.
    ///
    /// Confirmed cancel while already paused does not journal. A late
    /// non-cancel terminal is journaled; if the script becomes Active again,
    /// the run is immediately paused so resume kicks the next yield.
    pub(crate) fn apply_owned_host_result(&mut self, result: HostCallResult) -> Result<(), String> {
        if !self.pending_yield_started
            || self.pending_spawn_task_name.is_some()
            || !matches!(self.status, WorkflowStatus::Active | WorkflowStatus::Paused)
        {
            return Ok(());
        }
        if self.status == WorkflowStatus::Paused {
            if !result.ok && result.error == HOST_ERROR_TURN_CANCELLED {
                self.clear_pending_yield_started();
                return Ok(());
            }
            self.consume_pending_yield(result)?;
            if self.status == WorkflowStatus::Active {
                self.stop()?;
            }
            return Ok(());
        }
        if self.pending_kind == Some(ContinuationKind::Ask) && !result.ok {
            if result.error == HOST_ERROR_TURN_CANCELLED {
                self.stop()?;
                self.clear_pending_yield_started();
            } else {
                let error = if result.error.is_empty() {
                    HOST_ERROR_TURN_ERRORED.to_string()
                } else {
                    result.error.clone()
                };
                self.fail(error);
            }
            return Ok(());
        }
        self.advance_with_outcome(result).map(|_| ())
    }

    fn consume_pending_yield(
        &mut self,
        mut result: HostCallResult,
    ) -> Result<WorkflowAdvance, String> {
        let Some(_) = self.pending_instruction.as_ref() else {
            return Err("workflow has no pending yield".to_string());
        };
        let Some(kind) = self.pending_kind else {
            return Err("workflow has no pending yield identity".to_string());
        };
        let Some(digest) = self.pending_request_digest.clone() else {
            return Err("workflow has no pending yield identity".to_string());
        };
        result.text = truncate_workflow_reply(&result.text);
        self.push_continuation(kind, digest, result);
        self.pending_instruction = None;
        self.pending_spawn_task_name = None;
        self.pending_yield_started = false;
        self.pending_kind = None;
        self.pending_request_digest = None;
        match self.eval_current() {
            Ok(outcome) => self.apply_outcome(outcome),
            Err(error) => {
                self.fail(unrecoverable_error_code(&error));
                Ok(WorkflowAdvance::Failed)
            }
        }
    }

    pub fn occupies_idle(&self) -> bool {
        self.status == WorkflowStatus::Active
    }

    pub fn prepare_restored(&mut self) -> Result<(), String> {
        if bounded(&self.continuations).is_err() {
            self.fail(WORKFLOW_ERROR_UNSAFE_JOURNAL);
            return Ok(());
        }
        let has_legacy = !self.served_replies.is_empty() || self.served_pauses > 0;
        if !self.continuations.is_empty() {
            self.format_version = WORKFLOW_PERSIST_VERSION;
            self.served_asks = result_bearing_count(&self.continuations);
            return Ok(());
        }
        if matches!(
            self.status,
            WorkflowStatus::Complete | WorkflowStatus::Failed
        ) || !has_legacy
        {
            self.format_version = WORKFLOW_PERSIST_VERSION;
            return Ok(());
        }
        self.fail(WORKFLOW_ERROR_LEGACY_RESUME);
        Ok(())
    }

    pub(crate) fn unreadable(thread_id: ThreadId) -> Self {
        let now = unix_seconds();
        Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: "workflow".to_string(),
            status: WorkflowStatus::Failed,
            source: String::new(),
            served_asks: 0,
            served_replies: Vec::new(),
            served_pauses: 0,
            format_version: WORKFLOW_PERSIST_VERSION,
            continuations: Vec::new(),
            pending_kind: None,
            pending_request_digest: None,
            args: serde_json::Map::new(),
            phase: None,
            log: None,
            result: serde_json::Value::Null,
            error: Some(WORKFLOW_ERROR_UNSAFE_JOURNAL.to_string()),
            spawn_available: false,
            pending_spawn_task_name: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
            scratch_dir: None,
        }
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = WorkflowStatus::Failed;
        self.error = Some(error.into());
        self.pending_instruction = None;
        self.pending_spawn_task_name = None;
        self.pending_yield_started = false;
        self.pending_kind = None;
        self.pending_request_digest = None;
        self.updated_at = unix_seconds();
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if !matches!(
            self.status,
            WorkflowStatus::Active | WorkflowStatus::Waiting
        ) {
            return Err("workflow is not active".to_string());
        }
        self.status = WorkflowStatus::Paused;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn clear_pending_yield_started(&mut self) {
        self.pending_yield_started = false;
        self.updated_at = unix_seconds();
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Paused {
            return Err("workflow is not paused".to_string());
        }
        if self.pending_yield_started {
            return Err("workflow host turn is still in flight".to_string());
        }
        self.status = WorkflowStatus::Active;
        self.updated_at = unix_seconds();
        if self.pending_instruction.is_some() {
            return Ok(());
        }
        let previous = self.continuations.clone();
        let previous_asks = self.served_asks;
        let pending_kind = self.pending_kind;
        let pending_request_digest = self.pending_request_digest.clone();
        if let (Some(kind), Some(digest)) = (pending_kind, pending_request_digest.clone()) {
            self.push_continuation(kind, digest, HostCallResult::success(String::new()));
            self.pending_kind = None;
            self.pending_request_digest = None;
        }
        match self.eval_current() {
            Ok(outcome) => self.apply_outcome(outcome).map(|_| ()),
            Err(error) => {
                self.continuations = previous;
                self.served_asks = previous_asks;
                self.fail(unrecoverable_error_code(&error));
                Ok(())
            }
        }
    }

    pub fn mark_pending_yield_started(&mut self) {
        self.pending_yield_started = true;
        self.updated_at = unix_seconds();
    }

    fn eval_current(
        &self,
    ) -> Result<crate::engine::WorkflowEvalOutcome, crate::engine::WorkflowSourceError> {
        let args = json_args_to_map(&self.args);
        let spawn = if self.spawn_available {
            SpawnBinding::Available
        } else {
            SpawnBinding::Unavailable
        };
        match self.scratch_dir.as_deref() {
            Some(dir) => eval_source_with_scratch_and_spawn(
                &self.source,
                &self.continuations,
                &args,
                dir,
                spawn,
            ),
            None if self.spawn_available => {
                eval_source_with_spawn(&self.source, &self.continuations, &args, spawn)
            }
            None => eval_source_with_env(&self.source, &self.continuations, &args),
        }
    }

    fn apply_outcome(
        &mut self,
        outcome: crate::engine::WorkflowEvalOutcome,
    ) -> Result<WorkflowAdvance, String> {
        self.phase = outcome.phase;
        self.log = outcome.log;
        self.pending_spawn_task_name = match &outcome.eval {
            WorkflowEval::Yielded { .. } => outcome.spawn_task_name,
            WorkflowEval::Completed | WorkflowEval::Paused => None,
        };
        self.pending_kind = outcome.yield_kind;
        self.pending_request_digest = outcome.yield_request_digest;
        match outcome.eval {
            WorkflowEval::Completed => {
                self.status = WorkflowStatus::Complete;
                self.result = outcome.result;
                self.pending_instruction = None;
                self.pending_yield_started = false;
                self.updated_at = unix_seconds();
                Ok(WorkflowAdvance::Completed)
            }
            WorkflowEval::Yielded { instruction } => {
                self.status = WorkflowStatus::Active;
                self.pending_instruction = Some(instruction);
                self.pending_yield_started = false;
                self.updated_at = unix_seconds();
                Ok(WorkflowAdvance::Yielded)
            }
            WorkflowEval::Paused => {
                self.status = WorkflowStatus::Paused;
                self.pending_instruction = None;
                self.pending_yield_started = false;
                self.updated_at = unix_seconds();
                Ok(WorkflowAdvance::Paused)
            }
        }
    }

    fn push_continuation(
        &mut self,
        kind: ContinuationKind,
        request_digest: String,
        result: HostCallResult,
    ) {
        self.continuations.push(ContinuationRecord {
            seq: u32::try_from(self.continuations.len().saturating_add(1)).unwrap_or(u32::MAX),
            kind,
            request_digest,
            result,
        });
        self.served_asks = result_bearing_count(&self.continuations);
        self.format_version = WORKFLOW_PERSIST_VERSION;
    }
}

fn unrecoverable_error_code(error: &WorkflowSourceError) -> &'static str {
    match error {
        WorkflowSourceError::Invalid { reason } if reason.contains(REPLAY_DIVERGENCE) => {
            WORKFLOW_ERROR_REPLAY_DIVERGED
        }
        WorkflowSourceError::Invalid { .. }
        | WorkflowSourceError::Empty
        | WorkflowSourceError::TooLarge { .. } => WORKFLOW_ERROR_HOST_RUNTIME,
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "run_owned_host_tests.rs"]
mod owned_host_tests;
