use super::format_workflow_summary;
use super::workflow_status_indicator_from_workflow;
use crate::bottom_pane::WorkflowStatusIndicator;
use crate::bottom_pane::workflow_status_indicator_line;
use codex_app_server_protocol::ThreadWorkflow;
use codex_app_server_protocol::ThreadWorkflowStatus;

fn workflow(status: ThreadWorkflowStatus, instruction: Option<&str>) -> ThreadWorkflow {
    ThreadWorkflow {
        thread_id: "thread".to_string(),
        run_id: "run".to_string(),
        name: "workflow".to_string(),
        status,
        pending_instruction: instruction.map(str::to_string),
        created_at: 1,
        updated_at: 1,
    }
}

fn line_text(indicator: WorkflowStatusIndicator) -> String {
    workflow_status_indicator_line(Some(&indicator))
        .expect("indicator")
        .spans
        .into_iter()
        .map(|span| span.content.to_string())
        .collect()
}

#[test]
fn catalog_name_tokens_are_kebab_case() {
    assert!(super::looks_like_workflow_name("demo"));
    assert!(super::looks_like_workflow_name("deep-research.rhai"));
    assert!(!super::looks_like_workflow_name(r#"ask("x"); complete();"#));
    assert!(!super::looks_like_workflow_name("Upper"));
}

#[test]
fn workflow_summary_includes_yield_when_active() {
    insta::assert_snapshot!(
        format_workflow_summary(&workflow(
            ThreadWorkflowStatus::Active,
            Some("Compile the crate."),
        )),
        @r"
    Workflow: workflow (active)
    Yield: Compile the crate.
    "
    );
}

#[test]
fn workflow_summary_omits_yield_when_complete() {
    insta::assert_snapshot!(
        format_workflow_summary(&workflow(ThreadWorkflowStatus::Complete, None)),
        @"Workflow: workflow (complete)"
    );
}

#[test]
fn workflow_indicator_formats_each_status() {
    assert_eq!(
        workflow_status_indicator_from_workflow(&workflow(ThreadWorkflowStatus::Active, None)),
        WorkflowStatusIndicator::Active
    );
    insta::assert_snapshot!(
        line_text(WorkflowStatusIndicator::Active),
        @"Workflow active"
    );
    insta::assert_snapshot!(
        line_text(WorkflowStatusIndicator::Paused),
        @"Workflow paused (/workflow resume)"
    );
    insta::assert_snapshot!(
        line_text(WorkflowStatusIndicator::Complete),
        @"Workflow complete"
    );
    insta::assert_snapshot!(
        line_text(WorkflowStatusIndicator::Waiting),
        @"Workflow waiting"
    );
}
