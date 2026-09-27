use anyhow::Context;
use anyhow::Result;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::StructuredEditToolType;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

fn write_command_hook(
    home: &Path,
    script_name: &str,
    log_name: &str,
    event_name: &str,
    matcher: &str,
    allow_decision: Option<&str>,
) -> Result<Value> {
    let script_path = home.join(script_name);
    let log_path = home.join(log_name);
    let script = format!(
        r#"import json
from pathlib import Path
import sys
payload = json.load(sys.stdin)
with Path(r"{log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")
{decision}
"#,
        log_path = log_path.display(),
        decision = allow_decision.unwrap_or("pass"),
    );
    fs::write(&script_path, script).context("write hook script")?;
    Ok(json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": format!("python3 {}", script_path.display()),
            "statusMessage": format!("running {event_name} hook"),
        }]
    }))
}

fn write_structured_edit_identity_hooks(home: &Path) -> Result<()> {
    let permission_allow = r#"print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PermissionRequest",
        "decision": {"behavior": "allow"}
    }
}))"#;
    let hooks = json!({
        "hooks": {
            "PreToolUse": [write_command_hook(
                home,
                "pre_tool_use_hook.py",
                "pre_tool_use_hook_log.jsonl",
                "PreToolUse",
                "^structured_edit$",
                None,
            )?],
            "PermissionRequest": [write_command_hook(
                home,
                "permission_request_hook.py",
                "permission_request_hook_log.jsonl",
                "PermissionRequest",
                "^structured_edit$",
                Some(permission_allow),
            )?],
            "PostToolUse": [write_command_hook(
                home,
                "post_tool_use_hook.py",
                "post_tool_use_hook_log.jsonl",
                "PostToolUse",
                "^structured_edit$",
                None,
            )?],
        }
    });
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_apply_patch_only_permission_hook(home: &Path) -> Result<()> {
    let permission_allow = r#"print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PermissionRequest",
        "decision": {"behavior": "allow"}
    }
}))"#;
    let hooks = json!({
        "hooks": {
            "PermissionRequest": [write_command_hook(
                home,
                "permission_request_hook.py",
                "permission_request_hook_log.jsonl",
                "PermissionRequest",
                "^(apply_patch|Write|Edit)$",
                Some(permission_allow),
            )?],
        }
    });
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn read_hook_log(home: &Path, name: &str) -> Result<Vec<Value>> {
    fs::read_to_string(home.join(name))
        .with_context(|| format!("read {name}"))?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse hook log line"))
        .collect()
}

fn structured_edit_args() -> String {
    json!({
        "file_path": "nested/dir/hook.txt",
        "old_string": "old",
        "new_string": "new",
        "replace_all": false,
    })
    .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_hooks_keep_identity_across_pre_permission_post() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "structured-edit-hooks";
    let arguments = structured_edit_args();
    mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "structured_edit", &arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_model_info_override("gpt-5.4", |model| {
            model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
        })
        .with_pre_build_hook(|home| {
            write_structured_edit_identity_hooks(home)
                .expect("failed to write structured_edit hook fixtures");
        })
        .with_config(trust_discovered_hooks);
    let test = builder.build(&server).await?;
    fs::create_dir_all(test.workspace_path("nested/dir"))?;
    fs::write(test.workspace_path("nested/dir/hook.txt"), "old\n")?;

    test.submit_turn_with_approval_and_permission_profile(
        "edit nested/dir/hook.txt",
        AskForApproval::UnlessTrusted,
        PermissionProfile::Disabled,
    )
    .await?;

    assert_eq!(
        fs::read_to_string(test.workspace_path("nested/dir/hook.txt"))?,
        "new\n"
    );

    let expected_input = serde_json::from_str::<Value>(&arguments)?;
    for (log_name, event_name) in [
        ("pre_tool_use_hook_log.jsonl", "PreToolUse"),
        ("permission_request_hook_log.jsonl", "PermissionRequest"),
        ("post_tool_use_hook_log.jsonl", "PostToolUse"),
    ] {
        let inputs = read_hook_log(test.codex_home_path(), log_name)?;
        assert_eq!(inputs.len(), 1, "{log_name}");
        assert_eq!(inputs[0]["hook_event_name"], event_name);
        assert_eq!(inputs[0]["tool_name"], "structured_edit");
        assert_eq!(inputs[0]["tool_input"], expected_input);
        assert_ne!(inputs[0]["tool_input"]["file_path"], "hook.txt");
        assert!(inputs[0]["tool_input"].get("command").is_none());
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_permission_request_does_not_match_apply_patch_only_hook() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "structured-edit-apply-patch-hook";
    let arguments = structured_edit_args();
    mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "structured_edit", &arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_model_info_override("gpt-5.4", |model| {
            model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
        })
        .with_pre_build_hook(|home| {
            write_apply_patch_only_permission_hook(home)
                .expect("failed to write apply_patch-only hook fixture");
        })
        .with_config(trust_discovered_hooks);
    let test = builder.build(&server).await?;
    fs::create_dir_all(test.workspace_path("nested/dir"))?;
    fs::write(test.workspace_path("nested/dir/hook.txt"), "old\n")?;

    test.codex
        .start_or_steer_turn(
            codex_core::TurnInputRequest::user_input(vec![
                codex_protocol::user_input::UserInput::Text {
                    text: "edit nested/dir/hook.txt".into(),
                    text_elements: Vec::new(),
                },
            ])
            .with_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::UnlessTrusted),
                sandbox_policy: Some(codex_protocol::protocol::SandboxPolicy::DangerFullAccess),
                permission_profile: Some(PermissionProfile::Disabled),
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;

    let event = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::ApplyPatchApprovalRequest(_) | EventMsg::TurnComplete(_)
        )
    })
    .await;
    let EventMsg::ApplyPatchApprovalRequest(approval) = event else {
        panic!("apply_patch-only hook must not auto-approve structured_edit");
    };
    assert_eq!(approval.call_id, call_id);
    assert!(
        !test
            .codex_home_path()
            .join("permission_request_hook_log.jsonl")
            .exists(),
        "apply_patch-only PermissionRequest hook must not observe structured_edit"
    );
    test.codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Denied {
                rejection: "unmatched hook".to_string(),
            },
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(
        fs::read_to_string(test.workspace_path("nested/dir/hook.txt"))?,
        "old\n"
    );
    Ok(())
}
