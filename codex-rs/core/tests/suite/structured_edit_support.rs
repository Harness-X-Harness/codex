//! Shared completion and effect assertions for the exact-editor integration cases.

use anyhow::Context;
use anyhow::Result;
use codex_exec_server::CreateDirectoryOptions;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::PatchApplyStatus;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::wait_for_event;
use serde_json::Value;
use std::path::Path;

/// Own the selected cwd's diff root before the turn, independent of ancestor
/// repositories. Use that executor's ordinary fixture API, never an edit RPC.
pub(super) async fn establish_diff_root(
    test: &TestCodex,
    environment_id: &str,
    cwd: &Path,
) -> Result<()> {
    test.thread_manager
        .environment_manager()
        .get_environment(environment_id)
        .context("selected fixture environment")?
        .get_filesystem()
        .create_directory(
            &PathUri::from_host_native_path(cwd.join(".git"))?,
            CreateDirectoryOptions {
                recursive: false,
                follow_symlinks: true,
            },
            /*sandbox*/ None,
        )
        .await?;
    Ok(())
}

pub(super) async fn finish_turn(test: &TestCodex) -> Vec<EventMsg> {
    let mut events = Vec::new();
    wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        match event {
            EventMsg::TurnComplete(completed) => {
                assert!(completed.error.is_none(), "{:?}", completed.error);
                true
            }
            EventMsg::ApplyPatchApprovalRequest(_) => {
                panic!("unexpected approval request after the expected decision")
            }
            _ => false,
        }
    })
    .await;
    events
}

pub(super) fn assert_no_commit(events: &[EventMsg]) {
    for event in events {
        match event {
            EventMsg::ItemCompleted(event) => {
                if let TurnItem::FileChange(item) = &event.item {
                    assert_ne!(item.status, Some(PatchApplyStatus::Completed));
                }
            }
            EventMsg::PatchApplyEnd(event) => assert!(!event.success),
            EventMsg::TurnDiff(event) => assert!(event.unified_diff.is_empty()),
            _ => {}
        }
    }
}

pub(super) async fn mount_edit(
    server: &wiremock::MockServer,
    call: Value,
) -> responses::ResponseMock {
    responses::mount_sse_sequence(
        server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("edit-response"),
                call,
                responses::ev_completed("edit-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "done"),
                responses::ev_completed("final-response"),
            ]),
        ],
    )
    .await
}
