use codex_app_server_protocol::ThreadWorkflow;
use codex_app_server_protocol::ThreadWorkflowStatus;

use crate::bottom_pane::WorkflowStatusIndicator;

pub(crate) const WORKFLOW_USAGE: &str = "Usage: /workflow [start <rhai-or-path>|next|stop|resume]";

pub(crate) fn format_workflow_summary(workflow: &ThreadWorkflow) -> String {
    let status = match workflow.status {
        ThreadWorkflowStatus::Active => "active",
        ThreadWorkflowStatus::Paused => "paused",
        ThreadWorkflowStatus::Complete => "complete",
    };
    match workflow.steps.first() {
        Some(step) => format!(
            "Workflow: {} ({status})\nYield: {}",
            workflow.name, step.instruction
        ),
        None => format!("Workflow: {} ({status})", workflow.name),
    }
}

pub(crate) fn workflow_status_indicator_from_workflow(
    workflow: &ThreadWorkflow,
) -> WorkflowStatusIndicator {
    match workflow.status {
        ThreadWorkflowStatus::Active => WorkflowStatusIndicator::Active,
        ThreadWorkflowStatus::Paused => WorkflowStatusIndicator::Paused,
        ThreadWorkflowStatus::Complete => WorkflowStatusIndicator::Complete,
    }
}

#[cfg(test)]
#[path = "workflow_display_tests.rs"]
mod tests;
