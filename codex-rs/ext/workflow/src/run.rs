//! Host-owned workflow run state.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::spec::WorkflowDefinition;
use crate::spec::WorkflowStep;

/// Lifecycle of one thread's workflow run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStatus {
    Active,
    Paused,
    Complete,
}

/// Persisted run for one thread.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub thread_id: ThreadId,
    pub run_id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub current_step_index: usize,
    pub steps: Vec<WorkflowStep>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Result of advancing the current step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowAdvance {
    Advanced,
    Completed,
}

impl WorkflowRun {
    pub fn start(thread_id: ThreadId, definition: WorkflowDefinition) -> Self {
        let now = unix_seconds();
        Self {
            thread_id,
            run_id: Uuid::now_v7().to_string(),
            name: definition.name,
            status: WorkflowStatus::Active,
            current_step_index: 0,
            steps: definition.steps,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn current_step(&self) -> Option<&WorkflowStep> {
        self.steps.get(self.current_step_index)
    }

    pub fn advance(&mut self) -> Result<WorkflowAdvance, String> {
        if self.status != WorkflowStatus::Active {
            return Err("workflow is not active".to_string());
        }
        let last_index = self.steps.len().saturating_sub(1);
        if self.current_step_index >= last_index {
            self.status = WorkflowStatus::Complete;
            self.updated_at = unix_seconds();
            return Ok(WorkflowAdvance::Completed);
        }
        self.current_step_index += 1;
        self.updated_at = unix_seconds();
        Ok(WorkflowAdvance::Advanced)
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
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
