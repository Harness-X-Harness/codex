//! Shared completion and effect assertions for the exact-editor integration cases.

use anyhow::Context;
use anyhow::Result;
use codex_core::config::Config;
use codex_exec_server::CreateDirectoryOptions;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolName;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::wait_for_event;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug)]
struct ObservedToolOutcome {
    turn_id: String,
    call_id: String,
    tool_name: ToolName,
    outcome: ToolCallOutcome,
}

/// The provider wire omits a string output's success metadata. Observe the
/// existing typed lifecycle instead of treating nonempty prose as rejection.
#[derive(Default)]
pub(super) struct ToolOutcomes(Mutex<Vec<ObservedToolOutcome>>);

impl ToolLifecycleContributor for ToolOutcomes {
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("tool outcomes")
                .push(ObservedToolOutcome {
                    turn_id: input.turn_id.to_owned(),
                    call_id: input.call_id.to_owned(),
                    tool_name: input.tool_name.clone(),
                    outcome: input.outcome,
                });
        })
    }
}

impl ToolOutcomes {
    pub(super) fn observe(builder: TestCodexBuilder) -> (TestCodexBuilder, Arc<Self>) {
        let outcomes = Arc::new(Self::default());
        let mut extensions = ExtensionRegistryBuilder::<Config>::new();
        extensions.tool_lifecycle_contributor(outcomes.clone());
        (
            builder.with_extensions(Arc::new(extensions.build())),
            outcomes,
        )
    }

    pub(super) fn assert_failed(&self, turn_id: &str, call_id: &str, tool_name: &str) {
        let outcomes = self.0.lock().expect("tool outcomes");
        let bound = outcomes
            .iter()
            .filter(|outcome| outcome.turn_id == turn_id && outcome.call_id == call_id)
            .collect::<Vec<_>>();
        assert!(
            !bound.is_empty(),
            "missing failure for {turn_id}/{call_id}: {outcomes:?}"
        );
        for observed in bound {
            assert!(observed.tool_name.is_default_namespace());
            assert_eq!(observed.tool_name.name, tool_name);
            assert!(
                matches!(observed.outcome, ToolCallOutcome::Failed { .. }),
                "the bound invocation must fail, not merely leave bytes unchanged: {observed:?}"
            );
        }
    }

    pub(super) fn assert_edit_failed(&self, events: &[EventMsg], call_id: &str) {
        let turn_id = events
            .iter()
            .find_map(|event| match event {
                EventMsg::TurnComplete(turn) => Some(turn.turn_id.as_str()),
                _ => None,
            })
            .expect("completed turn");
        self.assert_failed(turn_id, call_id, "structured_edit");
    }
}

#[test]
fn success_without_effect_is_not_a_rejection() {
    // A tool could return nonempty success prose without changing any file.
    // Neither those bytes nor absent wire metadata can satisfy this witness.
    for outcome in [
        ToolCallOutcome::Completed { success: true },
        ToolCallOutcome::Completed { success: false },
        ToolCallOutcome::Blocked,
        ToolCallOutcome::Aborted,
    ] {
        let observed = ToolOutcomes(Mutex::new(vec![ObservedToolOutcome {
            turn_id: "turn".into(),
            call_id: "invalid".into(),
            tool_name: ToolName::plain("structured_edit"),
            outcome,
        }]));
        assert!(
            std::panic::catch_unwind(|| observed.assert_failed(
                "turn",
                "invalid",
                "structured_edit"
            ))
            .is_err()
        );
    }
    for handler_executed in [false, true] {
        let observed = ToolOutcomes(Mutex::new(vec![ObservedToolOutcome {
            turn_id: "turn".into(),
            call_id: "invalid".into(),
            tool_name: ToolName::plain("structured_edit"),
            outcome: ToolCallOutcome::Failed { handler_executed },
        }]));
        observed.assert_failed("turn", "invalid", "structured_edit");
    }
}

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

/// Compare this single-file fixture's target and changed lines, not Git header
/// spelling, hunk context, or the timing of aggregate-diff notifications.
pub(super) fn assert_edit_diff(diff: &str, target: &Path, environment: &str, edit: &str) {
    for prefix in ["--- ", "+++ "] {
        let headers = diff
            .lines()
            .filter_map(|line| line.strip_prefix(prefix))
            .collect::<Vec<_>>();
        assert_eq!(headers.len(), 1, "one existing-file update: {diff}");
        let header = headers[0];
        let header = header.split('\t').next().expect("diff path");
        let decoded = if header.starts_with('"') {
            serde_json::from_str::<String>(header).expect("quoted fixture diff path")
        } else {
            header.to_owned()
        };
        let path = decoded
            .strip_prefix("a/")
            .or_else(|| decoded.strip_prefix("b/"))
            .unwrap_or(&decoded);
        let prefix = format!("{environment}/");
        let path = path.strip_prefix(&prefix).unwrap_or(path);
        let path = Path::new(path)
            .components()
            .filter(|component| !matches!(component, std::path::Component::CurDir))
            .collect::<std::path::PathBuf>();
        let resolved = if path.is_absolute() {
            path
        } else {
            target.parent().expect("fixture root").join(path)
        };
        assert_eq!(resolved, target, "diff must describe the committed target");
    }
    assert_edit_changes(diff, edit);
}

pub(super) fn assert_edit_changes(actual: &str, expected: &str) {
    let changed_lines = |text: &str| {
        let removed = text
            .lines()
            .filter(|line| line.starts_with('-') && !line.starts_with("--- "))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let added = text
            .lines()
            .filter(|line| line.starts_with('+') && !line.starts_with("+++ "))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        (removed, added)
    };
    let expected = changed_lines(expected);
    assert!(
        !expected.0.is_empty() || !expected.1.is_empty(),
        "edit must describe an actual change"
    );
    assert_eq!(
        changed_lines(actual),
        expected,
        "diff must describe the committed edit"
    );
}

pub(super) fn output_request<'a>(
    requests: &'a [responses::ResponsesRequest],
    call_id: &str,
) -> &'a responses::ResponsesRequest {
    requests
        .iter()
        .rev()
        .find(|request| {
            ["function_call_output", "custom_tool_call_output"]
                .iter()
                .any(|kind| {
                    request
                        .inputs_of_type(kind)
                        .iter()
                        .any(|item| item["call_id"] == call_id)
                })
        })
        .expect("model continuation must contain the bound tool result")
}

/// Check only the wire surface: status can be omitted, so this does not prove
/// rejection. Failure cases also require a typed Failed/Declined witness;
/// successful cases require the actual committed lifecycle and bytes.
pub(super) fn assert_function_result(
    request: &responses::ResponsesRequest,
    call_id: &str,
    succeeded: bool,
) -> String {
    let (output, success) = request
        .function_call_output_content_and_success(call_id)
        .expect("bound function result");
    assert_ne!(
        success,
        Some(!succeeded),
        "contradictory result for {call_id}: {output:?}"
    );
    let output = output.expect("tool result text");
    assert!(
        !output.trim().is_empty(),
        "tool result must remain observable"
    );
    output
}

pub(super) fn assert_completed_edit(events: &[EventMsg], call_id: &str, target: &Path) {
    let turn_id = events
        .iter()
        .find_map(|event| match event {
            EventMsg::TurnComplete(turn) => Some(turn.turn_id.as_str()),
            _ => None,
        })
        .expect("completed turn");
    let mut completed = false;
    let mut edit = None;
    for event in events {
        if let EventMsg::ItemCompleted(event) = event
            && let TurnItem::FileChange(item) = &event.item
        {
            assert_eq!(event.turn_id, turn_id);
            assert_eq!(item.id, call_id, "only the bound logical edit may complete");
            assert_eq!(item.status, Some(PatchApplyStatus::Completed));
            assert_eq!(item.changes.len(), 1);
            let Some(FileChange::Update {
                unified_diff,
                move_path: None,
            }) = item.changes.get(target)
            else {
                panic!("completed edit must update only the bound existing file");
            };
            if let Some(previous) = edit {
                assert_edit_changes(unified_diff, previous);
            }
            edit = Some(unified_diff.as_str());
            completed = true;
        }
    }
    assert!(completed, "bound edit must have a completed public result");
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
