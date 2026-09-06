use codex_app_server_protocol::ThreadWorkflow;
use codex_app_server_protocol::ThreadWorkflowStatus;

use crate::bottom_pane::WorkflowStatusIndicator;

pub(crate) const WORKFLOW_USAGE: &str =
    "Usage: /workflow [start <name|rhai-or-path>|next|stop|resume]";

pub(crate) fn looks_like_workflow_name(token: &str) -> bool {
    let stem = token.trim().strip_suffix(".rhai").unwrap_or(token.trim());
    let bytes = stem.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes
            .last()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !bytes.windows(2).any(|pair| pair == b"--")
}

pub(crate) fn format_workflow_summary(workflow: &ThreadWorkflow) -> String {
    let status = match workflow.status {
        ThreadWorkflowStatus::Active => "active",
        ThreadWorkflowStatus::Paused => "paused",
        ThreadWorkflowStatus::Complete => "complete",
        ThreadWorkflowStatus::Waiting => "waiting",
        ThreadWorkflowStatus::Failed => "failed",
    };
    match workflow.pending_instruction.as_deref() {
        Some(instruction) if !instruction.is_empty() => format!(
            "Workflow: {} ({status})\nYield: {instruction}",
            workflow.name
        ),
        _ => format!("Workflow: {} ({status})", workflow.name),
    }
}

pub(crate) fn workflow_status_indicator_from_workflow(
    workflow: &ThreadWorkflow,
) -> WorkflowStatusIndicator {
    match workflow.status {
        ThreadWorkflowStatus::Active => WorkflowStatusIndicator::Active,
        ThreadWorkflowStatus::Paused => WorkflowStatusIndicator::Paused,
        ThreadWorkflowStatus::Complete => WorkflowStatusIndicator::Complete,
        ThreadWorkflowStatus::Waiting => WorkflowStatusIndicator::Waiting,
        ThreadWorkflowStatus::Failed => WorkflowStatusIndicator::Failed,
    }
}

#[cfg(test)]
#[path = "workflow_display_tests.rs"]
mod tests;
