use codex_app_server_protocol::ThreadWorkflow;
use codex_app_server_protocol::ThreadWorkflowStatus;

pub(crate) const WORKFLOW_USAGE: &str =
    "Usage: /workflow [start <markdown-or-path>|next|stop|resume]";

pub(crate) fn format_workflow_summary(workflow: &ThreadWorkflow) -> String {
    let status = match workflow.status {
        ThreadWorkflowStatus::Active => "active",
        ThreadWorkflowStatus::Paused => "paused",
        ThreadWorkflowStatus::Complete => "complete",
    };
    let current = workflow
        .steps
        .get(workflow.current_step_index as usize)
        .map(|step| step.title.as_str())
        .unwrap_or("none");
    let step_number = workflow.current_step_index.saturating_add(1);
    let step_count = workflow.steps.len();
    format!(
        "Workflow: {} ({status})\nStep {step_number}/{step_count}: {current}",
        workflow.name
    )
}
