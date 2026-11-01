use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::STALE_STRUCTURED_EDIT_MESSAGE;
use codex_login::CodexAuth;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ReviewDecision;
use codex_tools::ToolName;
use codex_utils_path_uri::PathUri;
use core_test_support::PathBufExt;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

use crate::apply_patch::convert_apply_patch_to_protocol;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use crate::state::ActiveTurn;
use crate::tools::orchestrator::ToolOrchestrator;
use crate::tools::runtimes::apply_patch::ApplyPatchHookIdentity;
use crate::tools::runtimes::apply_patch::ApplyPatchRequest;
use crate::tools::runtimes::apply_patch::ApplyPatchRuntime;
use crate::tools::runtimes::apply_patch::ApplyPatchWriteMode;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::ToolCtx;

#[tokio::test]
async fn structured_edit_native_read_denial_retry_keeps_original_snapshot() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().canonicalize().expect("canonical root");
    let target = root.join("target.txt");
    std::fs::write(&target, "original\r\n").expect("seed target");
    let target_uri = PathUri::from_host_native_path(&target).expect("target URI");
    let cwd = PathUri::from_host_native_path(&root).expect("cwd URI");
    let action = ApplyPatchAction::from_exact_update(
        cwd,
        target_uri.clone(),
        "original\r\n",
        "planned\r\n".to_owned(),
    );
    let changes = Arc::new(convert_apply_patch_to_protocol(&action));
    let (session, turn, events) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("Test API Key"),
        Vec::new(),
        |config| {
            config.cwd = root.abs();
            config.permissions.approval_policy =
                codex_config::Constrained::allow_any(AskForApproval::OnRequest);
            config.approvals_reviewer = ApprovalsReviewer::User;
            config
                .permissions
                .set_permission_profile(PermissionProfile::read_only())
                .expect("restricted write profile");
        },
    )
    .await;
    *session.active_turn.lock().await = Some(ActiveTurn::default());
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let turn_environment = step_context
        .environments
        .primary()
        .expect("Local environment")
        .clone();
    assert!(!turn_environment.environment.is_remote());
    assert_eq!(
        turn_environment.permission_profile(),
        &PermissionProfile::read_only()
    );
    let request = ApplyPatchRequest {
        turn_environment,
        action,
        file_paths: vec![target_uri],
        changes: Arc::clone(&changes),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
        write_mode: ApplyPatchWriteMode::VerifiedContents,
        hook_identity: ApplyPatchHookIdentity::StructuredEdit {
            arguments: serde_json::json!({
                "file_path": "target.txt",
                "old_string": "original",
                "new_string": "planned",
            }),
        },
    };
    let tool_ctx = ToolCtx {
        session: Arc::clone(&session),
        step_context,
        cancellation_token: CancellationToken::new(),
        call_id: "structured-read-denial".to_owned(),
        tool_name: ToolName::plain("structured_edit"),
    };
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000))
        .expect("deny native read");
    assert_eq!(
        std::fs::read(&target)
            .expect_err("fixture requires a real native read denial")
            .kind(),
        io::ErrorKind::PermissionDenied,
    );
    let mut runtime = ApplyPatchRuntime::new();
    let mut orchestrator = ToolOrchestrator::new();
    let operation = orchestrator.run(&mut runtime, &request, &tool_ctx);
    tokio::pin!(operation);

    // The event is the causal boundary: initial approval was skipped, so this
    // can only be the orchestrator's approval after its actual denied read.
    let event = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::select! {
            result = &mut operation => match result {
                Ok(_) => panic!("expected a retry approval before completion"),
                Err(error) => panic!("expected retry approval, got {error:?}"),
            },
            event = events.recv() => event.expect("retry approval event"),
        }
    })
    .await
    .expect("retry approval watchdog");
    let EventMsg::ApplyPatchApprovalRequest(approval) = event.msg else {
        panic!("expected the real patch retry approval");
    };
    assert_eq!(approval.call_id, tool_ctx.call_id);
    assert_eq!(approval.changes, *changes);
    assert_eq!(
        approval.reason.as_deref(),
        Some("command failed; retry without sandbox?")
    );
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
        .expect("restore read access");
    assert_eq!(
        std::fs::read(&target).expect("pre-retry bytes"),
        b"original\r\n"
    );
    std::fs::write(&target, "concurrent\r\n").expect("change while retry approval is pending");
    session
        .notify_approval(&approval.call_id, ReviewDecision::Approved)
        .await;

    let result = tokio::time::timeout(Duration::from_secs(30), operation)
        .await
        .expect("retry completion watchdog")
        .expect("runtime reports stale failure through its output");
    assert_eq!(result.output.exec_output.exit_code, 1);
    assert!(result.output.exec_output.stdout.text.is_empty());
    assert!(
        result
            .output
            .exec_output
            .stderr
            .text
            .contains(STALE_STRUCTURED_EDIT_MESSAGE)
    );
    assert!(result.output.delta.is_empty());
    assert!(!result.output.delta.is_exact());
    assert!(result.deferred_network_approval.is_none());
    assert_eq!(
        std::fs::read(&target).expect("final target bytes"),
        b"concurrent\r\n"
    );
    assert!(
        events.try_recv().is_err(),
        "no further approval or mutation event"
    );
}
