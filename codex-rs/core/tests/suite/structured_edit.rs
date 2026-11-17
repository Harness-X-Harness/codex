//! Provider-neutral Responses tests pinned to the real Local executor. These
//! tests deliberately have no network/remote skip gate: a missing prerequisite
//! must fail the required Linux proof, rather than count an unexecuted body.

use std::fs;
use std::path::Path;

use anyhow::Result;
use codex_apply_patch::STALE_STRUCTURED_EDIT_MESSAGE;
use codex_core::TurnInputRequest;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::StructuredEditToolType;
use codex_protocol::protocol::ApplyPatchApprovalRequestEvent;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PatchApplyStatus;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_apply_patch_custom_tool_call;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::oneshot;

use super::structured_edit_support::ToolOutcomes;
use super::structured_edit_support::assert_completed_edit;
use super::structured_edit_support::assert_edit_changes;
use super::structured_edit_support::assert_edit_diff;
use super::structured_edit_support::assert_function_result;
use super::structured_edit_support::assert_no_commit;
use super::structured_edit_support::establish_diff_root;
use super::structured_edit_support::finish_turn;
use super::structured_edit_support::output_request;

pub(super) fn structured_edit_builder() -> TestCodexBuilder {
    test_codex().with_model_info_override("gpt-5.4", |model| {
        model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
        model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
    })
}

async fn harness() -> Result<TestCodexHarness> {
    // Remote composition has its own actual exec-server fixture. A process-wide
    // remote test setting must not change the executor proved by this module.
    Box::pin(TestCodexHarness::with_builder(structured_edit_builder())).await
}

fn args(file_path: &str, old_string: &str, new_string: &str) -> Value {
    json!({"file_path": file_path, "old_string": old_string, "new_string": new_string})
}

async fn mount_edit(harness: &TestCodexHarness, call_id: &str, arguments: &Value) -> ResponseMock {
    super::structured_edit_support::mount_edit(
        harness.server(),
        ev_function_call(call_id, "structured_edit", &arguments.to_string()),
    )
    .await
}

async fn start_edit(
    test: &TestCodex,
    approval_policy: AskForApproval,
    permission_profile: PermissionProfile,
) -> Result<()> {
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(permission_profile, test.cwd_path());
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "perform the requested edit".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(approval_policy),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;
    Ok(())
}

async fn approval(test: &TestCodex, call_id: &str) -> ApplyPatchApprovalRequestEvent {
    let event = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::ApplyPatchApprovalRequest(_) | EventMsg::TurnComplete(_)
        )
    })
    .await;
    let EventMsg::ApplyPatchApprovalRequest(approval) = event else {
        panic!("structured_edit must request approval before completing");
    };
    assert_eq!(approval.call_id, call_id);
    assert!(!approval.turn_id.is_empty());
    approval
}

fn output(mock: &ResponseMock, call_id: &str) -> String {
    let requests = mock.requests();
    output_request(&requests, call_id)
        .function_call_output_text(call_id)
        .expect("paired structured_edit function output")
}

fn result(mock: &ResponseMock, call_id: &str, succeeded: bool) -> String {
    let requests = mock.requests();
    assert_function_result(output_request(&requests, call_id), call_id, succeeded)
}

/// Installs real generic hooks. Each map entry is a complete response for that
/// event; absent entries log the event and return an empty, non-blocking object.
pub(super) fn write_editor_hooks(home: &Path, matcher: &str, outputs: &Value) -> Result<()> {
    let script = home.join("editor_hooks.py");
    let responses = home.join("editor-hook-responses.json");
    fs::write(&responses, serde_json::to_vec(outputs)?)?;
    fs::write(
        &script,
        r#"import json
from pathlib import Path
import sys

home = Path(__file__).parent
payload = json.load(sys.stdin)
with (home / "editor-hooks.jsonl").open("a", encoding="utf-8") as log:
    log.write(json.dumps(payload) + "\n")
responses = json.loads((home / "editor-hook-responses.json").read_text())
print(json.dumps(responses.get(payload["hook_event_name"], {})))
"#,
    )?;
    let mut hooks = serde_json::Map::new();
    for event in ["PreToolUse", "PermissionRequest", "PostToolUse"] {
        hooks.insert(
            event.to_string(),
            json!([{
                "matcher": matcher,
                "hooks": [{"type": "command", "command": format!("python3 '{}'", script.display())}],
            }]),
        );
    }
    fs::write(home.join("hooks.json"), json!({"hooks": hooks}).to_string())?;
    Ok(())
}

pub(super) fn read_editor_hook_inputs(home: &Path) -> Result<Vec<Value>> {
    fs::read_to_string(home.join("editor-hooks.jsonl"))?
        .lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

async fn hooked_harness(outputs: Value) -> Result<TestCodexHarness> {
    let builder = structured_edit_builder()
        .with_pre_build_hook(move |home| {
            write_editor_hooks(home, "^structured_edit$", &outputs).expect("install editor hooks");
        })
        .with_config(trust_discovered_hooks);
    Box::pin(TestCodexHarness::with_builder(builder)).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_exact_bytes_lifecycle_and_continuation_history() -> Result<()> {
    let harness = harness().await?;
    establish_diff_root(
        harness.test(),
        codex_exec_server::LOCAL_ENVIRONMENT_ID,
        harness.test().cwd_path(),
    )
    .await?;
    let arguments = args("exact.txt", "α\r\n", "β\r\n");
    harness
        .write_file("exact.txt", "α\r\nuntouched\r\ntail")
        .await?;
    let call_id = "exact-edit";
    let mock = mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-edit"),
                ev_function_call(call_id, "structured_edit", &arguments.to_string()),
                ev_completed("resp-edit"),
            ]),
            sse(vec![
                ev_assistant_message("msg-done", "done"),
                ev_completed("resp-done"),
            ]),
            sse(vec![
                ev_assistant_message("msg-later", "still done"),
                ev_completed("resp-later"),
            ]),
        ],
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    let started = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemStarted(event) => match &event.item {
                TurnItem::FileChange(item) => Some((event.turn_id.clone(), item)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    let completed = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemCompleted(event) => match &event.item {
                TurnItem::FileChange(item) => Some((event.turn_id.clone(), item)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!started.is_empty(), "edit must start");
    assert!(!completed.is_empty(), "edit must complete");
    let completed_turn_id = events
        .iter()
        .find_map(|event| match event {
            EventMsg::TurnComplete(completed) => Some(completed.turn_id.as_str()),
            _ => None,
        })
        .expect("completed turn");
    let path = harness.path("exact.txt");
    for (turn_id, item) in &started {
        assert_eq!(turn_id, completed_turn_id);
        assert_eq!(item.id, call_id);
        assert_eq!(item.changes.len(), 1);
        assert!(matches!(
            item.changes.get(&path),
            Some(FileChange::Update {
                move_path: None,
                ..
            })
        ));
    }
    for (turn_id, item) in &completed {
        assert_eq!(turn_id, completed_turn_id);
        assert_eq!(item.id, call_id);
        assert_eq!(item.status, Some(PatchApplyStatus::Completed));
        assert_eq!(item.changes.len(), 1);
        assert!(matches!(
            item.changes.get(&path),
            Some(FileChange::Update {
                move_path: None,
                ..
            })
        ));
    }
    let Some(FileChange::Update {
        unified_diff,
        move_path,
    }) = completed[0].1.changes.get(&path)
    else {
        panic!("expected one existing-file update for the Local target");
    };
    assert_eq!(move_path, &None);
    assert!(
        unified_diff.contains("-α") && unified_diff.contains("+β"),
        "{unified_diff}"
    );
    for (_, item) in &completed {
        let Some(FileChange::Update {
            unified_diff: repeated,
            ..
        }) = item.changes.get(&path)
        else {
            unreachable!("validated target");
        };
        assert_edit_changes(repeated, unified_diff);
    }
    let committed_diff = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::TurnDiff(diff) => Some(diff.unified_diff.as_str()),
            _ => None,
        })
        .next_back()
        .expect("committed edit must emit a turn diff");
    assert_edit_diff(committed_diff, &path, "local", unified_diff);
    assert_eq!(fs::read(&path)?, "β\r\nuntouched\r\ntail".as_bytes());

    harness
        .submit_with_permission_profile("describe the completed edit", PermissionProfile::Disabled)
        .await?;
    let requests = mock.requests();
    output_request(&requests, call_id);
    for request in &requests[1..] {
        let calls = request.inputs_of_type("function_call");
        let calls = calls
            .iter()
            .filter(|item| item["call_id"] == call_id)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["name"], "structured_edit");
        assert_eq!(
            serde_json::from_str::<Value>(calls[0]["arguments"].as_str().expect("arguments"))?,
            arguments
        );
        assert_eq!(request.inputs_of_type("function_call_output").len(), 1);
        assert_function_result(request, call_id, true);
    }
    assert_eq!(fs::read(path)?, "β\r\nuntouched\r\ntail".as_bytes());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_cardinality_and_invalid_arguments_leave_no_commit() -> Result<()> {
    let mut malformed_replace_all = args("keep.txt", "aa", "bb");
    malformed_replace_all["replace_all"] = json!("true");
    let mut unknown_field = args("keep.txt", "aa", "bb");
    unknown_field["unexpected"] = json!(true);
    for arguments in [
        args("keep.txt", "absent", "new"),
        args("keep.txt", "aa aa\n", "new"),
        args("keep.txt", "aa", "bb"),
        args("keep.txt", "", "new"),
        args("keep.txt", "aa", "aa"),
        malformed_replace_all,
        unknown_field,
    ] {
        let (builder, outcomes) = ToolOutcomes::observe(structured_edit_builder());
        let harness = Box::pin(TestCodexHarness::with_builder(builder)).await?;
        harness.write_file("keep.txt", b"aa aa\r\ntail").await?;
        let mock = mount_edit(&harness, "invalid-edit", &arguments).await;
        start_edit(
            harness.test(),
            AskForApproval::Never,
            PermissionProfile::Disabled,
        )
        .await?;
        let events = finish_turn(harness.test()).await;
        assert_no_commit(&events);
        outcomes.assert_edit_failed(&events, "invalid-edit");
        result(&mock, "invalid-edit", false);
        assert_eq!(fs::read(harness.path("keep.txt"))?, b"aa aa\r\ntail");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_replace_all_uses_nonoverlapping_exact_matches() -> Result<()> {
    let harness = harness().await?;
    harness.write_file("overlap.txt", "aaaaa\r\nαα").await?;
    let mut arguments = args("overlap.txt", "aa", "β");
    arguments["replace_all"] = json!(true);
    let mock = mount_edit(&harness, "replace-all", &arguments).await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    result(&mock, "replace-all", true);
    assert_completed_edit(&events, "replace-all", &harness.path("overlap.txt"));
    assert_eq!(
        fs::read(harness.path("overlap.txt"))?,
        "ββa\r\nαα".as_bytes()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_missing_directory_and_invalid_utf8_are_rejected() -> Result<()> {
    for target in ["missing.txt", "directory", "invalid.txt"] {
        let (builder, outcomes) = ToolOutcomes::observe(structured_edit_builder());
        let harness = Box::pin(TestCodexHarness::with_builder(builder)).await?;
        harness.create_dir_all("directory").await?;
        harness
            .write_file("invalid.txt", [0xff, 0xfe, b'a'])
            .await?;
        let mock = mount_edit(&harness, "ineligible-edit", &args(target, "a", "b")).await;
        start_edit(
            harness.test(),
            AskForApproval::Never,
            PermissionProfile::Disabled,
        )
        .await?;
        let events = finish_turn(harness.test()).await;
        assert_no_commit(&events);
        outcomes.assert_edit_failed(&events, "ineligible-edit");
        result(&mock, "ineligible-edit", false);
        assert!(!harness.path("missing.txt").exists());
        assert!(harness.path("directory").is_dir());
        assert_eq!(fs::read(harness.path("invalid.txt"))?, [0xff, 0xfe, b'a']);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_approved_snapshot_commits_and_denial_preserves_bytes() -> Result<()> {
    for decision in [
        ReviewDecision::Approved,
        ReviewDecision::Denied {
            rejection: "fixture denied".to_string(),
        },
    ] {
        let harness = harness().await?;
        harness.write_file("approval.txt", "before\r\ntail").await?;
        let mock = mount_edit(
            &harness,
            "approved-edit",
            &args("approval.txt", "before", "after"),
        )
        .await;
        start_edit(
            harness.test(),
            AskForApproval::UnlessTrusted,
            PermissionProfile::Disabled,
        )
        .await?;
        let approval = approval(harness.test(), "approved-edit").await;
        assert_eq!(
            approval.changes.keys().collect::<Vec<_>>(),
            vec![&harness.path("approval.txt")]
        );
        let approved = matches!(decision, ReviewDecision::Approved);
        harness
            .test()
            .codex
            .submit(Op::PatchApproval {
                id: approval.call_id,
                decision,
            })
            .await?;
        let events = finish_turn(harness.test()).await;
        if approved {
            result(&mock, "approved-edit", true);
            assert_completed_edit(&events, "approved-edit", &harness.path("approval.txt"));
            assert_eq!(fs::read(harness.path("approval.txt"))?, b"after\r\ntail");
        } else {
            assert_no_commit(&events);
            assert!(events.iter().any(|event| matches!(event, EventMsg::ItemCompleted(event) if matches!(&event.item, TurnItem::FileChange(item) if item.id == "approved-edit" && item.status == Some(PatchApplyStatus::Declined)))));
            result(&mock, "approved-edit", false);
            assert_eq!(fs::read(harness.path("approval.txt"))?, b"before\r\ntail");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_stale_approval_retains_competing_bytes_and_empty_diff() -> Result<()> {
    let harness = harness().await?;
    harness.write_file("stale.txt", "before\n").await?;
    let mock = mount_edit(
        &harness,
        "stale-edit",
        &args("stale.txt", "before", "planned"),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;
    let approval = approval(harness.test(), "stale-edit").await;
    harness.write_file("stale.txt", "competing\n").await?;
    harness
        .test()
        .codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Approved,
        })
        .await?;
    let events = finish_turn(harness.test()).await;
    assert_no_commit(&events);
    assert!(events.iter().any(|event| matches!(event, EventMsg::ItemCompleted(event) if matches!(&event.item, TurnItem::FileChange(item) if item.id == "stale-edit" && item.status == Some(PatchApplyStatus::Failed)))));
    let output = output(&mock, "stale-edit");
    assert!(output.contains(STALE_STRUCTURED_EDIT_MESSAGE), "{output}");
    assert_eq!(fs::read(harness.path("stale.txt"))?, b"competing\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_cancellation_while_awaiting_approval_has_no_effect() -> Result<()> {
    let harness = harness().await?;
    harness.write_file("cancel.txt", "before\n").await?;
    let mock = mount_sse_once(
        harness.server(),
        sse(vec![
            ev_response_created("resp-cancel"),
            ev_function_call(
                "cancel-edit",
                "structured_edit",
                &args("cancel.txt", "before", "after").to_string(),
            ),
            ev_completed("resp-cancel"),
        ]),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;
    approval(harness.test(), "cancel-edit").await;
    harness.test().codex.submit(Op::Interrupt).await?;
    let mut events = Vec::new();
    let terminal = wait_for_event(&harness.test().codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
    })
    .await;
    assert!(matches!(terminal, EventMsg::TurnAborted(_)));
    assert_no_commit(&events);
    harness.test().codex.shutdown_and_wait().await?;
    assert_eq!(fs::read(harness.path("cancel.txt"))?, b"before\n");
    mock.single_request();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_target_removed_after_approval_is_not_recreated() -> Result<()> {
    let harness = harness().await?;
    harness.write_file("removed.txt", "before\n").await?;
    let mock = mount_edit(
        &harness,
        "removed-edit",
        &args("removed.txt", "before", "after"),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;
    let approval = approval(harness.test(), "removed-edit").await;
    fs::remove_file(harness.path("removed.txt"))?;
    harness
        .test()
        .codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Approved,
        })
        .await?;
    assert_no_commit(&finish_turn(harness.test()).await);
    let output = output(&mock, "removed-edit");
    assert!(!output.contains("Success. Updated"), "{output}");
    assert!(!harness.path("removed.txt").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_write_failure_after_approval_does_not_publish_planned_diff() -> Result<()>
{
    let harness = harness().await?;
    harness.write_file("readonly.txt", "before\n").await?;
    let mock = mount_edit(
        &harness,
        "write-failed",
        &args("readonly.txt", "before", "after"),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;
    let approval = approval(harness.test(), "write-failed").await;
    let path = harness.path("readonly.txt");
    let original_permissions = fs::metadata(&path)?.permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&path, read_only)?;
    // Fail if the runner bypasses fixture permissions. A privileged process
    // cannot turn this write-denial control into a meaningful passing result.
    let prerequisite = fs::OpenOptions::new().write(true).open(&path);
    assert_eq!(
        prerequisite
            .expect_err("proof requires an unprivileged runner")
            .kind(),
        std::io::ErrorKind::PermissionDenied,
    );
    harness
        .test()
        .codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Approved,
        })
        .await?;
    let events = finish_turn(harness.test()).await;
    fs::set_permissions(&path, original_permissions)?;
    assert_no_commit(&events);
    let output = output(&mock, "write-failed");
    assert!(output.contains("Failed to write file"), "{output}");
    assert!(output.contains("Permission denied"), "{output}");
    assert!(!output.contains("Success. Updated"), "{output}");
    assert_eq!(fs::read(path)?, b"before\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_read_only_planning_policy_rejects_before_commit() -> Result<()> {
    let harness = harness().await?;
    harness.write_file("policy.txt", "before\n").await?;
    let mock = mount_edit(
        &harness,
        "policy-edit",
        &args("policy.txt", "before", "after"),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::read_only(),
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    assert_no_commit(&events);
    assert!(!events.iter().any(|event| matches!(event, EventMsg::ItemStarted(event) if matches!(&event.item, TurnItem::FileChange(_)))));
    let output = output(&mock, "policy-edit");
    assert!(
        output.contains("patch rejected: writing is blocked by read-only sandbox"),
        "{output}"
    );
    assert_eq!(fs::read(harness.path("policy.txt"))?, b"before\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_hooks_rewrite_effective_json_for_permission_and_post() -> Result<()> {
    let original = args("original.txt", "before", "ignored");
    let mut effective = args("rewritten.txt", "before", "accepted");
    effective["environment_id"] = json!(codex_exec_server::LOCAL_ENVIRONMENT_ID);
    let post_context = "The exact editor hook observed committed bytes.";
    let harness = hooked_harness(json!({
        "PreToolUse": {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": effective}},
        "PermissionRequest": {"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}},
        "PostToolUse": {"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": post_context}},
    })).await?;
    harness.write_file("original.txt", "before\n").await?;
    harness.write_file("rewritten.txt", "before\n").await?;
    let mock = mount_edit(&harness, "hook-edit", &original).await;
    start_edit(
        harness.test(),
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    let output = result(&mock, "hook-edit", true);
    assert_completed_edit(&events, "hook-edit", &harness.path("rewritten.txt"));
    assert_eq!(fs::read(harness.path("original.txt"))?, b"before\n");
    assert_eq!(fs::read(harness.path("rewritten.txt"))?, b"accepted\n");
    let hooks = read_editor_hook_inputs(harness.test().codex_home_path())?;
    assert_eq!(
        hooks
            .iter()
            .map(|input| input["hook_event_name"].clone())
            .collect::<Vec<_>>(),
        vec![
            json!("PreToolUse"),
            json!("PermissionRequest"),
            json!("PostToolUse")
        ]
    );
    for input in &hooks {
        assert_eq!(input["tool_name"], "structured_edit");
        assert_eq!(input["turn_id"], hooks[0]["turn_id"]);
        assert!(input["turn_id"].as_str().is_some_and(|id| !id.is_empty()));
    }
    assert_eq!(hooks[0]["tool_input"], original);
    assert_eq!(hooks[1]["tool_input"], effective);
    assert_eq!(hooks[2]["tool_input"], effective);
    assert_eq!(hooks[0]["tool_use_id"], "hook-edit");
    assert!(hooks[1].get("tool_use_id").is_none());
    assert_eq!(hooks[2]["tool_use_id"], "hook-edit");
    assert_eq!(hooks[2]["tool_response"], output);
    assert!(
        mock.requests()[1]
            .message_input_texts("developer")
            .contains(&post_context.to_string())
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_pre_and_permission_hook_denials_never_commit() -> Result<()> {
    for event_name in ["PreToolUse", "PermissionRequest"] {
        let response = if event_name == "PreToolUse" {
            json!({"hookSpecificOutput": {"hookEventName": event_name, "permissionDecision": "deny", "permissionDecisionReason": "editor fixture denied"}})
        } else {
            json!({"hookSpecificOutput": {"hookEventName": event_name, "decision": {"behavior": "deny", "message": "editor fixture denied"}}})
        };
        let mut outputs = json!({});
        outputs[event_name] = response;
        let harness = hooked_harness(outputs).await?;
        harness.write_file("denied.txt", "before\n").await?;
        let arguments = args("denied.txt", "before", "after");
        let mock = mount_edit(&harness, "hook-denied", &arguments).await;
        start_edit(
            harness.test(),
            AskForApproval::UnlessTrusted,
            PermissionProfile::Disabled,
        )
        .await?;
        assert_no_commit(&finish_turn(harness.test()).await);
        let output = output(&mock, "hook-denied");
        assert!(output.contains("editor fixture denied"), "{output}");
        assert!(!output.contains("Success. Updated"), "{output}");
        assert_eq!(fs::read(harness.path("denied.txt"))?, b"before\n");
        let hooks = read_editor_hook_inputs(harness.test().codex_home_path())?;
        let expected_events = if event_name == "PreToolUse" {
            vec![json!("PreToolUse")]
        } else {
            vec![json!("PreToolUse"), json!("PermissionRequest")]
        };
        assert_eq!(
            hooks
                .iter()
                .map(|input| input["hook_event_name"].clone())
                .collect::<Vec<_>>(),
            expected_events
        );
        for input in hooks {
            assert_eq!(input["tool_name"], "structured_edit");
            assert_eq!(input["tool_input"], arguments);
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_invalid_hook_rewrite_cannot_reuse_original_arguments() -> Result<()> {
    let original = args("keep.txt", "before", "after");
    let mut rewritten = original.clone();
    rewritten["replace_all"] = json!("true");
    let harness = hooked_harness(json!({"PreToolUse": {"hookSpecificOutput": {
        "hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": rewritten,
    }}}))
    .await?;
    harness.write_file("keep.txt", "before\n").await?;
    let mock = mount_edit(&harness, "invalid-rewrite", &original).await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    assert_no_commit(&finish_turn(harness.test()).await);
    assert!(output(&mock, "invalid-rewrite").contains("boolean"));
    assert_eq!(fs::read(harness.path("keep.txt"))?, b"before\n");
    let hooks = read_editor_hook_inputs(harness.test().codex_home_path())?;
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["tool_input"], original);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_post_hook_block_preserves_committed_effect_and_diff() -> Result<()> {
    let harness = hooked_harness(json!({
        "PostToolUse": {"decision": "block", "reason": "post editor gate blocked output"},
    }))
    .await?;
    harness.write_file("committed.txt", "a\n").await?;
    let arguments = args("committed.txt", "a", "aa");
    let mock = mount_edit(&harness, "post-blocked", &arguments).await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    let output = output(&mock, "post-blocked");
    assert!(
        output.contains("post editor gate blocked output"),
        "{output}"
    );
    assert!(!output.contains("Success. Updated"), "{output}");
    assert_eq!(fs::read(harness.path("committed.txt"))?, b"aa\n");
    assert_completed_edit(&events, "post-blocked", &harness.path("committed.txt"));
    assert!(events.iter().any(
        |event| matches!(event, EventMsg::TurnDiff(diff) if diff.unified_diff.contains("+aa"))
    ));
    let hooks = read_editor_hook_inputs(harness.test().codex_home_path())?;
    assert_eq!(hooks.len(), 2);
    assert_eq!(hooks[1]["hook_event_name"], "PostToolUse");
    assert_eq!(hooks[1]["tool_input"], arguments);
    assert!(
        !hooks[1]["tool_response"]
            .as_str()
            .expect("committed hook output")
            .trim()
            .is_empty()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_capability_keeps_stock_apply_patch_and_its_hooks() -> Result<()> {
    let builder = structured_edit_builder()
        .with_pre_build_hook(|home| {
            write_editor_hooks(home, "^apply_patch$", &json!({}))
                .expect("install stock patch hooks");
        })
        .with_config(trust_discovered_hooks);
    let harness = Box::pin(TestCodexHarness::with_builder(builder)).await?;
    harness.write_file("stock.txt", "before\n").await?;
    let patch = "*** Begin Patch\n*** Update File: stock.txt\n@@\n-before\n+after\n*** End Patch";
    let mock = super::structured_edit_support::mount_edit(
        harness.server(),
        ev_apply_patch_custom_tool_call("stock-patch", patch),
    )
    .await;
    start_edit(
        harness.test(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(harness.test()).await;
    assert_completed_edit(&events, "stock-patch", &harness.path("stock.txt"));
    assert_eq!(fs::read(harness.path("stock.txt"))?, b"after\n");
    let requests = mock.requests();
    let tools = requests[0].body_json()["tools"]
        .as_array()
        .expect("tools")
        .clone();
    assert!(tools.iter().any(|tool| tool["name"] == "structured_edit"));
    assert!(tools.iter().any(|tool| tool["name"] == "apply_patch"));
    let (stock_output, success) = output_request(&requests, "stock-patch")
        .custom_tool_call_output_content_and_success("stock-patch")
        .expect("bound stock output");
    assert_ne!(success, Some(false));
    assert!(
        stock_output
            .as_deref()
            .is_some_and(|output| !output.trim().is_empty())
    );
    let hooks = read_editor_hook_inputs(harness.test().codex_home_path())?;
    assert_eq!(hooks.len(), 2);
    for input in hooks {
        assert_eq!(input["tool_name"], "apply_patch");
        assert_eq!(input["tool_input"], json!({"command": patch}));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_stalled_stream_keeps_durable_output_without_reexecution() -> Result<()> {
    let call_id = "durable-edit";
    let arguments = args("retry.txt", "a", "aa");
    let (stall_tx, stall_rx) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: sse(vec![
                    ev_response_created("resp-stalled"),
                    ev_function_call(call_id, "structured_edit", &arguments.to_string()),
                ]),
            },
            StreamingSseChunk {
                gate: Some(stall_rx),
                body: sse(vec![ev_completed("resp-stalled")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: sse(vec![
                ev_response_created("resp-retry"),
                ev_assistant_message("msg-retry", "done"),
                ev_completed("resp-retry"),
            ]),
        }],
    ])
    .await;
    let mut builder = structured_edit_builder().with_config(|config| {
        config.model_provider.stream_max_retries = Some(1);
        config.model_provider.stream_idle_timeout_ms = Some(100);
    });
    let test = builder.build_with_streaming_server(&server).await?;
    fs::write(test.workspace_path("retry.txt"), b"a\n")?;
    start_edit(&test, AskForApproval::Never, PermissionProfile::Disabled).await?;
    let events = finish_turn(&test).await;
    assert_completed_edit(&events, call_id, &test.workspace_path("retry.txt"));
    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    let retry: Value = serde_json::from_slice(&requests[1])?;
    let input = retry["input"].as_array().expect("retry history");
    for item_type in ["function_call", "function_call_output"] {
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == item_type && item["call_id"] == call_id)
                .count(),
            1
        );
    }
    test.codex.ensure_rollout_materialized().await;
    test.codex.flush_rollout().await?;
    let rollout = fs::read_to_string(test.codex.rollout_path().expect("durable rollout path"))?;
    let persisted = rollout
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<serde_json::Result<Vec<_>>>()?;
    for item_type in ["function_call", "function_call_output"] {
        assert_eq!(
            persisted
                .iter()
                .filter(|line| line["type"] == "response_item"
                    && line["payload"]["type"] == item_type
                    && line["payload"]["call_id"] == call_id)
                .count(),
            1,
            "the paired edit history must be written exactly once to durable storage"
        );
    }
    assert_eq!(fs::read(test.workspace_path("retry.txt"))?, b"aa\n");
    let _ = stall_tx.send(());
    server.shutdown().await;
    Ok(())
}
