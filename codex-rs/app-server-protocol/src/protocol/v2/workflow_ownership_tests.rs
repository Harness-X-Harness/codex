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
