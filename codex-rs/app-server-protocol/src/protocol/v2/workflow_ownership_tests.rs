use super::ThreadWorkflow;
use super::ThreadWorkflowStatus;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn stock_thread_module_does_not_define_workflow_dtos() {
    let source = include_str!("thread.rs");
    for needle in [
        "pub enum ThreadWorkflowStatus",
        "pub struct ThreadWorkflow ",
        "pub struct ThreadWorkflowGetParams",
        "pub struct ThreadWorkflowGetResponse",
        "pub struct ThreadWorkflowStartParams",
        "pub struct ThreadWorkflowStartResponse",
        "pub struct ThreadWorkflowAdvanceParams",
        "pub struct ThreadWorkflowAdvanceResponse",
        "pub struct ThreadWorkflowStopParams",
        "pub struct ThreadWorkflowStopResponse",
        "pub struct ThreadWorkflowResumeParams",
        "pub struct ThreadWorkflowResumeResponse",
        "pub struct ThreadWorkflowUpdatedNotification",
    ] {
        assert!(
            !source.contains(needle),
            "stock v2/thread.rs still defines {needle}"
        );
    }
}

#[test]
fn workflow_status_and_error_wire_shape_is_unchanged() {
    let workflow = ThreadWorkflow {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        name: "demo".into(),
        status: ThreadWorkflowStatus::Failed,
        pending_instruction: None,
        result: json!(null),
        error: Some("host_runtime".into()),
        created_at: 1,
        updated_at: 2,
    };

    assert_eq!(
        serde_json::to_value(workflow).expect("serialize ThreadWorkflow"),
        json!({
            "threadId": "thread-1",
            "runId": "run-1",
            "name": "demo",
            "status": "failed",
            "pendingInstruction": null,
            "result": null,
            "error": "host_runtime",
            "createdAt": 1,
            "updatedAt": 2,
        })
    );
}
