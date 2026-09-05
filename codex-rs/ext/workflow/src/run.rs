//! Host-owned workflow run state.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::engine::WorkflowEval;
use crate::engine::eval_source;

/// Lifecycle of one thread's workflow run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStatus {
    Active,
    Paused,
    Complete,
}

/// Display projection of the current yield, if any.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub title: String,
    pub instruction: String,
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
}

impl WorkflowRun {
    pub fn start(thread_id: ThreadId, source: &str) -> Result<Self, String> {
        let now = unix_seconds();
        let mut run = Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: "workflow".to_string(),
            status: WorkflowStatus::Active,
            source: source.trim().to_string(),
            served_asks: 0,
            pending_instruction: None,
            pending_yield_started: false,
            created_at: now,
            updated_at: now,
        };
        run.apply_eval(eval_source(&run.source, 0).map_err(|error| error.to_string())?)?;
        Ok(run)
    }

    pub fn current_step(&self) -> Option<WorkflowStep> {
        self.display_steps().into_iter().next()
    }

    pub fn current_step_index(&self) -> usize {
        0
    }

    pub fn display_steps(&self) -> Vec<WorkflowStep> {
        match &self.pending_instruction {
            Some(instruction) => vec![WorkflowStep {
                id: "ask".to_string(),
                title: "ask".to_string(),
                instruction: instruction.clone(),
            }],
            None => Vec::new(),
        }
    }

    pub fn advance(&mut self) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        let Some(_) = self.pending_instruction.as_ref() else {
            return Err("workflow has no pending yield".to_string());
        };
        let served_asks = self.served_asks.saturating_add(1);
        let pending = self.pending_instruction.clone();
        let pending_yield_started = self.pending_yield_started;
        self.served_asks = served_asks;
        self.pending_instruction = None;
        self.pending_yield_started = false;
        match eval_source(&self.source, served_asks) {
            Ok(outcome) => self.apply_eval(outcome),
            Err(error) => {
                self.served_asks = served_asks.saturating_sub(1);
                self.pending_instruction = pending;
                self.pending_yield_started = pending_yield_started;
                Err(error.to_string())
            }
        }
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        self.status = WorkflowStatus::Paused;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.status != WorkflowStatus::Paused {
            return Err("workflow is not paused".to_string());
        }
        self.status = WorkflowStatus::Active;
        self.updated_at = unix_seconds();
        Ok(())
    }

    pub fn mark_pending_yield_started(&mut self) {
        self.pending_yield_started = true;
        self.updated_at = unix_seconds();
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
        }
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
