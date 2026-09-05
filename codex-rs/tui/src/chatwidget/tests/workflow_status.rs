use super::*;
use crate::bottom_pane::WorkflowStatusIndicator;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn thread_workflow_updated_sets_footer_indicator() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.set_feature_enabled(Feature::GoalHost, /*enabled*/ true);
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.handle_server_notification(
        ServerNotification::ThreadWorkflowUpdated(
            codex_app_server_protocol::ThreadWorkflowUpdatedNotification {
                thread_id: thread_id.to_string(),
                workflow: codex_app_server_protocol::ThreadWorkflow {
                    thread_id: thread_id.to_string(),
                    run_id: "run".to_string(),
                    name: "workflow".to_string(),
                    status: codex_app_server_protocol::ThreadWorkflowStatus::Active,
                    current_step_index: 0,
                    steps: vec![codex_app_server_protocol::ThreadWorkflowStep {
                        id: "ask".to_string(),
                        title: "ask".to_string(),
                        instruction: "Compile the crate.".to_string(),
                    }],
                    created_at: 1,
                    updated_at: 1,
                },
            },
        ),
        /*replay_kind*/ None,
    );

    assert_eq!(
        chat.current_workflow_status_indicator,
        Some(WorkflowStatusIndicator::Active)
    );
}

#[tokio::test]
async fn session_configured_clears_workflow_status_footer() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.set_feature_enabled(Feature::GoalHost, /*enabled*/ true);
    chat.current_workflow_status_indicator = Some(WorkflowStatusIndicator::Active);
    chat.bottom_pane
        .set_workflow_status_indicator(Some(WorkflowStatusIndicator::Active));

    let rollout_file = tempfile::NamedTempFile::new().unwrap();
    chat.handle_thread_session(crate::session_state::ThreadSessionState {
        thread_id: ThreadId::new(),
        forked_from_id: None,
        fork_parent_title: None,
        thread_name: None,
        model: "gpt-5.4".to_string(),
        model_provider_id: "test-provider".to_string(),
        service_tier: None,
        approval_policy: AskForApproval::Never,
        approvals_reviewer: ApprovalsReviewer::User,
        permission_profile: PermissionProfile::read_only(),
        active_permission_profile: None,
        cwd: test_path_buf("/home/user/project").abs(),
        runtime_workspace_roots: Vec::new(),
        instruction_source_paths: Vec::new(),
        reasoning_effort: Some(ReasoningEffortConfig::default()),
        collaboration_mode: None,
        personality: None,
        message_history: None,
        network_proxy: None,
        rollout_path: Some(rollout_file.path().to_path_buf()),
    });

    assert_eq!(chat.current_workflow_status_indicator, None);
}
