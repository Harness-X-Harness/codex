//! Shared completion and effect assertions for the exact-editor integration cases.

use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::PatchApplyStatus;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::wait_for_event;
use serde_json::Value;

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
