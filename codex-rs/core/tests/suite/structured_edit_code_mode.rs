//! Real Code Mode host/delegate composition, using controlled Responses input.

use anyhow::Context;
use anyhow::Result;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_path_uri::PathUri;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;

use super::code_mode::custom_tool_output_items;
use super::code_mode::scripted_json_witness;
use super::structured_edit::read_editor_hook_inputs;
use super::structured_edit::structured_edit_builder;
use super::structured_edit::write_editor_hooks;
use super::structured_edit_remote::assert_committed;
use super::structured_edit_remote::assert_remote_cas;
use super::structured_edit_remote::environment_request;
use super::structured_edit_remote::fixture::RemoteFixture;
use super::structured_edit_remote::fixture::editor_binary;
use super::structured_edit_remote::start_turn;
use super::structured_edit_support::assert_no_commit;
use super::structured_edit_support::establish_diff_root;
use super::structured_edit_support::finish_turn;
use super::structured_edit_support::mount_edit;
use super::structured_edit_support::output_request;

fn code_mode_builder() -> Result<TestCodexBuilder> {
    Ok(structured_edit_builder()
        .with_code_mode_host_program(editor_binary("codex-code-mode-host")?)
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodeMode)
                .expect("enable real Code Mode");
            config
                .features
                .enable(Feature::CodeModeHost)
                .expect("enable process host");
            config.code_mode.disable_in_process_fallback = true;
            config
                .features
                .enable(Feature::ExecutedToolCallMetadata)
                .expect("enable nested metadata");
        }))
}

fn nested_edit(args: &Value) -> String {
    format!(
        "try {{ text({{ witness: 'structured-edit', outcome: 'completed', result: await tools.structured_edit({args}) }}); }} catch (error) {{ text({{ witness: 'structured-edit', outcome: 'rejected', error: String(error) }}); }}"
    )
}

fn output(request: &responses::ResponsesRequest) -> Value {
    let (_, success) = request
        .custom_tool_call_output_content_and_success("exec-edit")
        .expect("actual Code Mode output");
    assert_ne!(success, Some(false), "the controlled script must finish");
    scripted_json_witness(
        &custom_tool_output_items(request, "exec-edit"),
        "structured-edit",
    )
}

fn assert_nested_outcome(result: &Value, expected: &str) {
    assert_eq!(result["outcome"], expected, "{result}");
    match expected {
        "completed" => assert!(result.get("error").is_none(), "{result}"),
        "rejected" => {
            assert!(result.get("result").is_none(), "{result}");
            assert!(
                result["error"]
                    .as_str()
                    .is_some_and(|error| !error.trim().is_empty()),
                "the caught rejection must remain observable: {result}"
            );
        }
        _ => panic!("unknown fixture outcome {expected}"),
    }
}

#[test]
fn nested_edit_witness_decodes_outcome_instead_of_matching_prose() {
    let completed = json!({"type": "input_text", "text": r#"{
        "witness": "structured\u002dedit", "outcome": "comple\u0074ed", "result": {}
    }"#});
    let rejected = json!({"type": "input_text", "text": json!({
        "witness": "structured-edit", "outcome": "rejected",
        "error": "completed and nested-completed are only words in this error"
    }).to_string()});
    let diagnostic = json!({"type": "input_text", "text": "host diagnostic: nested-completed"});
    let result = scripted_json_witness(&[diagnostic.clone(), completed.clone()], "structured-edit");
    assert_nested_outcome(&result, "completed");
    let result = scripted_json_witness(&[rejected.clone(), diagnostic.clone()], "structured-edit");
    assert_nested_outcome(&result, "rejected");
    assert!(std::panic::catch_unwind(|| assert_nested_outcome(&result, "completed")).is_err());
    for items in [
        vec![completed.clone(), rejected],
        vec![completed.clone(), completed],
        vec![diagnostic],
    ] {
        assert!(
            std::panic::catch_unwind(|| scripted_json_witness(&items, "structured-edit")).is_err()
        );
    }
    for result in [
        json!({"outcome":"rejected", "error":""}),
        json!({"outcome":"rejected", "error":"failed", "result":{}}),
    ] {
        assert!(std::panic::catch_unwind(|| assert_nested_outcome(&result, "rejected")).is_err());
    }
}

#[test_case("local", "before", "after"; "local_success")]
#[test_case("remote", "before", "after"; "remote_success")]
#[test_case("remote", "absent", "after"; "remote_exact_error")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_structured_edit_uses_real_delegate(
    environment: &str,
    old: &str,
    new: &str,
) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let test = code_mode_builder()?
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let local_path = test.workspace_path("same.txt");
    let remote_path = remote_root.path().join("same.txt");
    std::fs::write(&local_path, "before")?;
    std::fs::write(&remote_path, "before")?;
    let target = if environment == "remote" {
        &remote_path
    } else {
        &local_path
    };
    let target_uri = PathUri::from_host_native_path(target)?;
    establish_diff_root(&test, environment, target.parent().context("selected cwd")?).await?;
    fixture.observe_path(&target_uri);
    let args = json!({"file_path":"same.txt", "old_string":old, "new_string":new, "environment_id":environment});
    let code = nested_edit(&args);
    let response = mount_edit(
        &server,
        responses::ev_custom_tool_call("exec-edit", "exec", &code),
    )
    .await;
    start_turn(
        &test,
        vec![
            environment_request("local", test.cwd_path())?,
            environment_request("remote", remote_root.path())?,
        ],
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(&test).await;
    let requests = response.requests();
    let request = output_request(&requests, "exec-edit");
    let result = output(request);
    if old == "before" {
        assert_nested_outcome(&result, "completed");
        assert_committed(&events, target, environment);
        assert_eq!(std::fs::read_to_string(target)?, new);
        if environment == "remote" {
            assert_remote_cas(&fixture, &target_uri);
        }
    } else {
        assert_nested_outcome(&result, "rejected");
        assert_no_commit(&events);
        assert_eq!(std::fs::read_to_string(target)?, "before");
        assert!(
            !fixture
                .records()
                .iter()
                .any(|rpc| rpc.method.contains("writeFile"))
        );
    }
    let other = if environment == "remote" {
        &local_path
    } else {
        &remote_path
    };
    assert_eq!(std::fs::read_to_string(other)?, "before");
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}

#[test_case("approve"; "approved")]
#[test_case("deny"; "declined")]
#[test_case("stale"; "stale_after_approval")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_structured_edit_remote_approval_is_the_actual_edit_boundary(
    decision: &str,
) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let test = code_mode_builder()?
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let path = remote_root.path().join("same.txt");
    std::fs::write(&path, "before")?;
    let path_uri = PathUri::from_host_native_path(&path)?;
    establish_diff_root(&test, "remote", remote_root.path()).await?;
    fixture.observe_path(&path_uri);
    let code = nested_edit(
        &json!({"file_path":"same.txt", "old_string":"before", "new_string":"after", "environment_id":"remote"}),
    );
    let response = mount_edit(
        &server,
        responses::ev_custom_tool_call("exec-edit", "exec", &code),
    )
    .await;
    start_turn(
        &test,
        vec![
            environment_request("local", test.cwd_path())?,
            environment_request("remote", remote_root.path())?,
        ],
        AskForApproval::OnRequest,
        PermissionProfile::read_only(),
    )
    .await?;
    let approval = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ApplyPatchApprovalRequest(_))
    })
    .await;
    let EventMsg::ApplyPatchApprovalRequest(approval) = approval else {
        unreachable!()
    };
    assert_eq!(approval.changes.keys().collect::<Vec<_>>(), vec![&path]);
    assert_eq!(std::fs::read_to_string(&path)?, "before");
    assert!(
        !fixture
            .records()
            .iter()
            .any(|rpc| rpc.method.contains("writeFile"))
    );
    if decision == "stale" {
        std::fs::write(&path, "concurrent writer")?;
    }
    test.codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: if decision == "deny" {
                ReviewDecision::Denied {
                    rejection: "declined exact edit".to_owned(),
                }
            } else {
                ReviewDecision::Approved
            },
        })
        .await?;
    let events = finish_turn(&test).await;
    let requests = response.requests();
    let request = output_request(&requests, "exec-edit");
    let result = output(request);
    if decision == "approve" {
        assert_nested_outcome(&result, "completed");
        assert_committed(&events, &path, "remote");
        assert_eq!(std::fs::read_to_string(&path)?, "after");
        assert_remote_cas(&fixture, &path_uri);
    } else {
        assert_nested_outcome(&result, "rejected");
        assert_no_commit(&events);
        assert_eq!(
            std::fs::read_to_string(&path)?,
            if decision == "stale" {
                "concurrent writer"
            } else {
                "before"
            }
        );
        if decision == "stale" {
            assert!(
                result["error"]
                    .as_str()
                    .expect("caught rejection")
                    .contains(codex_apply_patch::STALE_STRUCTURED_EDIT_MESSAGE),
                "{result}"
            );
        }
        assert!(
            !fixture
                .records()
                .iter()
                .any(|rpc| rpc.method.contains("writeFile"))
        );
    }
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}

#[test_case("rewrite"; "pre_rewrites_remote_and_post_observes_effective_arguments")]
#[test_case("block"; "pre_blocks_before_remote_mutation")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_structured_edit_uses_generic_hooks(behavior: &str) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let effective = json!({"file_path":"same.txt", "old_string":"before", "new_string":"hook changed", "environment_id":"remote"});
    let pre = if behavior == "rewrite" {
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":effective}})
    } else {
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"blocked exact edit"}})
    };
    let hook_outputs = json!({"PreToolUse":pre,"PostToolUse":{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"exact edit observed"}}});
    let test = code_mode_builder()?
        .with_exec_server_url(fixture.url())
        .with_pre_build_hook(move |home| {
            write_editor_hooks(home, "^structured_edit$", &hook_outputs)
                .expect("editor hook fixture");
        })
        .with_config(trust_discovered_hooks)
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let local_path = test.workspace_path("same.txt");
    let remote_path = remote_root.path().join("same.txt");
    std::fs::write(&local_path, "before")?;
    std::fs::write(&remote_path, "before")?;
    let target_uri = PathUri::from_host_native_path(&remote_path)?;
    establish_diff_root(&test, "remote", remote_root.path()).await?;
    fixture.observe_path(&target_uri);
    let code = nested_edit(
        &json!({"file_path":"same.txt", "old_string":"before", "new_string":"unrewritten", "environment_id":"local"}),
    );
    let response = mount_edit(
        &server,
        responses::ev_custom_tool_call("exec-edit", "exec", &code),
    )
    .await;
    start_turn(
        &test,
        vec![
            environment_request("local", test.cwd_path())?,
            environment_request("remote", remote_root.path())?,
        ],
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(&test).await;
    let requests = response.requests();
    let request = output_request(&requests, "exec-edit");
    let result = output(request);
    let hooks = read_editor_hook_inputs(test.codex_home_path())?;
    assert_eq!(hooks[0]["hook_event_name"], "PreToolUse");
    assert_eq!(hooks[0]["tool_name"], "structured_edit");
    assert_eq!(hooks[0]["tool_input"]["environment_id"], "local");
    assert_eq!(std::fs::read_to_string(&local_path)?, "before");
    if behavior == "rewrite" {
        assert_committed(&events, &remote_path, "remote");
        assert_eq!(std::fs::read_to_string(&remote_path)?, "hook changed");
        assert_remote_cas(&fixture, &target_uri);
        let post = hooks
            .iter()
            .find(|input| input["hook_event_name"] == "PostToolUse")
            .context("post hook")?;
        assert_eq!(post["tool_name"], "structured_edit");
        assert_eq!(post["tool_input"], effective);
        assert!(
            request
                .message_input_texts("developer")
                .iter()
                .any(|text| text.contains("exact edit observed"))
        );
        assert_nested_outcome(&result, "completed");
    } else {
        assert_eq!(hooks.len(), 1);
        assert_eq!(std::fs::read_to_string(&remote_path)?, "before");
        assert_no_commit(&events);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EventMsg::PatchApplyEnd(_)))
        );
        assert!(
            !fixture
                .records()
                .iter()
                .any(|rpc| rpc.method.contains("writeFile"))
        );
        assert_nested_outcome(&result, "rejected");
        assert!(
            result["error"]
                .as_str()
                .expect("caught rejection")
                .contains("blocked exact edit")
        );
    }
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}
