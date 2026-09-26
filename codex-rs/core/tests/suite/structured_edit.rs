use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::REMOTE_ENVIRONMENT_ID;
use codex_exec_server::RemoveOptions;
use codex_exec_server::WriteFileOptions;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::StructuredEditToolType;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_protocol::user_input::UserInput;
use codex_utils_path_uri::PathUri;
use core_test_support::PathBufExt;
use core_test_support::PathExt;
use core_test_support::responses::ev_apply_patch_custom_tool_call;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_no_remote_env;
use core_test_support::skip_if_target_windows;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::local;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tempfile::TempDir;

async fn structured_edit_harness() -> Result<TestCodexHarness> {
    structured_edit_harness_with(|builder| builder).await
}

async fn structured_edit_harness_with(
    configure: impl FnOnce(TestCodexBuilder) -> TestCodexBuilder,
) -> Result<TestCodexHarness> {
    let builder = configure(test_codex().with_model_info_override("gpt-5.4", |model| {
        model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
    }));
    Box::pin(TestCodexHarness::with_auto_env_builder(builder)).await
}

fn structured_edit_args(
    file_path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> String {
    json!({
        "file_path": file_path,
        "old_string": old_string,
        "new_string": new_string,
        "replace_all": replace_all,
    })
    .to_string()
}

async fn mount_structured_edit(harness: &TestCodexHarness, call_id: &str, arguments: &str) {
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "structured_edit", arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
}

async fn submit_structured_edit(harness: &TestCodexHarness, prompt: &str) -> Result<()> {
    harness
        .submit_with_permission_profile(prompt, PermissionProfile::Disabled)
        .await
}

fn output_text(value: &serde_json::Value) -> String {
    value
        .get("output")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_replaces_one_match_and_emits_file_change() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = structured_edit_harness().await?;
    harness.write_file("hello.txt", "hello\r\nworld").await?;
    let call_id = "structured-edit-success";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("hello.txt", "hello", "hi", /*replace_all*/ false),
    )
    .await;

    let test = harness.test();
    let codex = test.codex.clone();
    let session_model = test.session_configured.model.clone();
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "edit hello.txt".into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(SandboxPolicy::DangerFullAccess),
                permission_profile: Some(PermissionProfile::Disabled),
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;

    let mut saw_begin = false;
    let mut end_success = None;
    let mut turn_diff = None;
    wait_for_event(&codex, |event| match event {
        EventMsg::PatchApplyBegin(_) => {
            saw_begin = true;
            false
        }
        EventMsg::PatchApplyEnd(end) => {
            end_success = Some(end.success);
            false
        }
        EventMsg::TurnDiff(ev) => {
            turn_diff = Some(ev.unified_diff.clone());
            false
        }
        EventMsg::TurnComplete(_) => true,
        _ => false,
    })
    .await;

    assert!(saw_begin, "expected PatchApplyBegin");
    assert_eq!(end_success, Some(true));
    let diff = turn_diff.expect("expected TurnDiff");
    assert!(diff.contains("hello.txt"), "{diff}");
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(
        output.contains("Success. Updated the following files:"),
        "{output}"
    );
    assert_eq!(harness.read_file_text("hello.txt").await?, "hi\r\nworld");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_zero_matches_does_not_mutate() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("keep.txt", "unchanged\n").await?;
    let call_id = "structured-edit-zero";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("keep.txt", "missing", "x", /*replace_all*/ false),
    )
    .await;
    submit_structured_edit(&harness, "edit keep.txt").await?;
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(output.contains("old_string was not found"), "{output}");
    assert!(!output.contains("Success. Updated"), "{output}");
    assert_eq!(harness.read_file_text("keep.txt").await?, "unchanged\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_multiple_matches_without_replace_all_does_not_mutate() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("dup.txt", "aa aa\n").await?;
    let call_id = "structured-edit-multi";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("dup.txt", "aa", "bb", /*replace_all*/ false),
    )
    .await;
    submit_structured_edit(&harness, "edit dup.txt").await?;
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(output.contains("matched 2 times"), "{output}");
    assert!(!output.contains("Success. Updated"), "{output}");
    assert_eq!(harness.read_file_text("dup.txt").await?, "aa aa\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_replace_all_replaces_every_match() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("dup.txt", "aa aa aa\n").await?;
    let call_id = "structured-edit-replace-all";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("dup.txt", "aa", "bb", /*replace_all*/ true),
    )
    .await;
    submit_structured_edit(&harness, "replace all").await?;
    assert_eq!(harness.read_file_text("dup.txt").await?, "bb bb bb\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_empty_old_string_does_not_create_or_overwrite() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("keep.txt", "seed\n").await?;
    let call_id = "structured-edit-empty-old";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("keep.txt", "", "created", /*replace_all*/ true),
    )
    .await;
    submit_structured_edit(&harness, "empty old_string").await?;
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(output.contains("old_string must be non-empty"), "{output}");
    assert_eq!(harness.read_file_text("keep.txt").await?, "seed\n");
    assert!(!harness.path_exists("created").await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_identical_strings_do_not_mutate() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("keep.txt", "same\n").await?;
    let call_id = "structured-edit-identical";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("keep.txt", "same", "same", /*replace_all*/ false),
    )
    .await;
    submit_structured_edit(&harness, "identical strings").await?;
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(
        output.contains("old_string and new_string must differ"),
        "{output}"
    );
    assert_eq!(harness.read_file_text("keep.txt").await?, "same\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_missing_file_does_not_create() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    let call_id = "structured-edit-missing";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("missing.txt", "a", "b", /*replace_all*/ false),
    )
    .await;
    submit_structured_edit(&harness, "missing file").await?;
    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(
        output.contains("can only edit existing text files"),
        "{output}"
    );
    assert!(!harness.path_exists("missing.txt").await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_approval_denied_does_not_mutate() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("deny.txt", "before\n").await?;
    let call_id = "structured-edit-denied";
    mount_structured_edit(
        &harness,
        call_id,
        &structured_edit_args("deny.txt", "before", "after", /*replace_all*/ false),
    )
    .await;

    let test = harness.test();
    let session_model = test.session_configured.model.clone();
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "edit deny.txt".into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::UnlessTrusted),
                sandbox_policy: Some(SandboxPolicy::DangerFullAccess),
                permission_profile: Some(PermissionProfile::Disabled),
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: session_model,
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
        panic!("expected structured_edit approval before completion");
    };
    assert_eq!(approval.call_id, call_id);
    test.codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Denied {
                rejection: "test denied".to_string(),
            },
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output = output_text(&harness.function_call_output_value(call_id).await);
    assert!(!output.contains("Success. Updated"), "{output}");
    assert_eq!(harness.read_file_text("deny.txt").await?, "before\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_does_not_change_stock_apply_patch() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let harness = structured_edit_harness().await?;
    harness.write_file("stock.txt", "before\n").await?;
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_apply_patch_custom_tool_call(
                    "stock-apply-patch",
                    "*** Begin Patch\n*** Update File: stock.txt\n@@\n-before\n+after\n*** End Patch",
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    submit_structured_edit(&harness, "use apply_patch").await?;
    assert_eq!(harness.read_file_text("stock.txt").await?, "after\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_routes_to_selected_remote_environment() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_target_windows!(Ok(()), "requires the Docker-backed POSIX executor");
    skip_if_no_remote_env!(Ok(()));

    let server = start_mock_server().await;
    let test = test_codex()
        .with_model_info_override("gpt-5.4", |model| {
            model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
        })
        .build_with_remote_and_local_env(&server)
        .await?;
    let local_cwd = TempDir::new()?;
    let file_name = "structured_edit_remote.txt";
    let remote_cwd = PathBuf::from(format!(
        "/tmp/codex-remote-structured-edit-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    ))
    .abs();
    let remote_cwd_uri = PathUri::from_host_native_path(&remote_cwd)?;
    test.fs()
        .create_directory(
            &remote_cwd_uri,
            CreateDirectoryOptions {
                recursive: true,
                follow_symlinks: true,
            },
            /*sandbox*/ None,
        )
        .await?;
    let remote_file = PathUri::from_host_native_path(remote_cwd.join(file_name))?;
    test.fs()
        .write_file(
            &remote_file,
            b"before\n".to_vec(),
            WriteFileOptions {
                follow_symlinks: true,
            },
            /*sandbox*/ None,
        )
        .await?;

    let call_id = "structured-edit-remote";
    let arguments = json!({
        "file_path": file_name,
        "old_string": "before",
        "new_string": "after",
        "replace_all": false,
        "environment_id": REMOTE_ENVIRONMENT_ID,
    })
    .to_string();
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

    test.submit_turn_with_environments(
        "edit the remote file",
        Some(vec![
            local(local_cwd.path().abs()),
            TurnEnvironmentSelection {
                environment_id: REMOTE_ENVIRONMENT_ID.to_string(),
                cwd: PathUri::from_abs_path(&remote_cwd),
                workspace_roots: vec![PathUri::from_abs_path(&remote_cwd)],
                config: EnvironmentConfigState::FromThread,
            },
        ]),
    )
    .await?;

    let remote_contents = test
        .fs()
        .read_file_text(&remote_file, Default::default(), /*sandbox*/ None)
        .await?;
    assert_eq!(remote_contents, "after\n");
    assert!(
        !local_cwd.path().join(file_name).exists(),
        "structured_edit should not write the remote file into the local environment"
    );

    test.fs()
        .remove(
            &remote_cwd_uri,
            RemoveOptions {
                recursive: true,
                force: true,
                follow_symlinks: true,
            },
            /*sandbox*/ None,
        )
        .await?;
    Ok(())
}
