//! Host-owned workflow run state.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::catalog::json_args_to_map;
use crate::engine::WorkflowEval;
use crate::engine::eval_source_with_env;
use crate::engine::truncate_workflow_reply;
use crate::engine::validate_source;

/// Lifecycle of one thread's workflow run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStatus {
    Active,
    Paused,
    Complete,
    Waiting,
}

/// Persisted run for one thread.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    pub args: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub phase: Option<String>,
    pub pending_instruction: Option<String>,
    /// True after the host started a model turn for the current yield.
    #[serde(default)]
    pub pending_yield_started: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Result of a host resume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowAdvance {
    Yielded,
    Completed,
    Paused,
}

impl WorkflowRun {
    pub fn start(thread_id: ThreadId, source: &str) -> Result<Self, String> {
        Self::start_named(thread_id, "workflow", source, serde_json::Map::new())
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
            args,
            phase: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
        };
        let outcome = run.eval_current().map_err(|error| error.to_string())?;
        run.phase = outcome.phase;
        run.apply_eval(outcome.eval)?;
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
            args,
            phase: None,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn activate(&mut self) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Waiting {
            return Err("workflow is not waiting".to_string());
        }
        let outcome = self.eval_current().map_err(|error| error.to_string())?;
        self.phase = outcome.phase;
        self.apply_eval(outcome.eval)
    }

    pub fn park(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Paused {
            return Err("workflow is not paused".to_string());
        }
        self.status = WorkflowStatus::Waiting;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn advance(&mut self) -> Result<WorkflowAdvance, String> {
        self.advance_with_reply(String::new())
    }

    pub fn advance_with_reply(&mut self, reply: String) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        let Some(_) = self.pending_instruction.as_ref() else {
            return Err("workflow has no pending yield".to_string());
        };
        let pending = self.pending_instruction.clone();
        let pending_yield_started = self.pending_yield_started;
        let previous_asks = self.served_asks;
        let previous_replies = self.served_replies.clone();
        let previous_pauses = self.served_pauses;
        self.served_replies.push(truncate_workflow_reply(&reply));
        self.served_asks = u32::try_from(self.served_replies.len()).unwrap_or(u32::MAX);
        self.pending_instruction = None;
        self.pending_yield_started = false;
        match self.eval_current() {
            Ok(outcome) => {
                self.phase = outcome.phase;
                self.apply_eval(outcome.eval)
            }
            Err(error) => {
                self.served_asks = previous_asks;
                self.served_replies = previous_replies;
                self.served_pauses = previous_pauses;
                self.pending_instruction = pending;
                self.pending_yield_started = pending_yield_started;
                Err(error.to_string())
            }
        }
    }

    pub fn occupies_idle(&self) -> bool {
        self.status == WorkflowStatus::Active
    }

    pub fn normalize_served_replies(&mut self) {
        let expected = usize::try_from(self.served_asks).unwrap_or(usize::MAX);
        if self.served_replies.len() < expected {
            self.served_replies.resize(expected, String::new());
        }
        self.served_asks = u32::try_from(self.served_replies.len()).unwrap_or(u32::MAX);
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if !matches!(
            self.status,
            WorkflowStatus::Active | WorkflowStatus::Waiting
        ) {
            return Err("workflow is not active".to_string());
        }
        self.status = WorkflowStatus::Paused;
        // Drop the in-flight yield claim so resume can kick a new host turn.
        self.pending_yield_started = false;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Paused {
            return Err("workflow is not paused".to_string());
        }
        self.status = WorkflowStatus::Active;
        self.updated_at = unix_seconds();
        if self.pending_instruction.is_some() {
            return Ok(());
        }
        let previous = self.served_pauses;
        self.served_pauses = self.served_pauses.saturating_add(1);
        match self.eval_current() {
            Ok(outcome) => {
                self.phase = outcome.phase;
                self.apply_eval(outcome.eval).map(|_| ())
            }
            Err(error) => {
                self.served_pauses = previous;
                self.status = WorkflowStatus::Paused;
                Err(error.to_string())
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
        eval_source_with_env(
            &self.source,
            &self.served_replies,
            self.served_pauses,
            &json_args_to_map(&self.args),
        )
    }

    fn apply_eval(&mut self, outcome: WorkflowEval) -> Result<WorkflowAdvance, String> {
        match outcome {
            WorkflowEval::Completed => {
                self.status = WorkflowStatus::Complete;
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
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
