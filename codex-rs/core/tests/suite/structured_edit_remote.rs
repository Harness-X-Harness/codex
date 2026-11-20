//! Provider-neutral editor routing through an independently running executor.
//! Cargo/Bazel supply real binaries; required Grok proof pins their paths and digests.
//! Missing prerequisites fail rather than silently skipping the test bodies.

#[path = "structured_edit_remote_fixture.rs"]
pub(super) mod fixture;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::ApplyPatchOptions;
use codex_apply_patch::apply_verified_action;
use codex_core::TurnInputRequest;
use codex_exec_server::Environment;
use codex_exec_server::EnvironmentObservedStatus;
use codex_exec_server::ExecServerRuntimeOptions;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_exec_server::LocalFileSystem;
use codex_exec_server::REMOTE_ENVIRONMENT_ID;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::FileSystemSpecialPath;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PatchApplyStatus;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::TurnEnvironmentRequest;
use codex_protocol::protocol::TurnEnvironmentRequests;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use fixture::RemoteFixture;
use fixture::RpcTerminal;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;

use super::structured_edit::structured_edit_builder;
use super::structured_edit_support::ToolOutcomes;
use super::structured_edit_support::assert_edit_changes;
use super::structured_edit_support::assert_edit_diff;
use super::structured_edit_support::assert_function_result;
use super::structured_edit_support::assert_no_commit;
use super::structured_edit_support::establish_diff_root;
use super::structured_edit_support::finish_turn;
use super::structured_edit_support::mount_edit;
use super::structured_edit_support::output_request;

pub(super) fn environment_request(id: &str, cwd: &Path) -> Result<TurnEnvironmentRequest> {
    let cwd = PathUri::from_host_native_path(cwd)?;
    Ok(TurnEnvironmentRequest {
        environment_id: id.to_owned(),
        cwd: cwd.clone(),
        workspace_roots: vec![cwd],
        config: EnvironmentConfigState::FromThread,
    })
}

pub(super) async fn start_turn(
    test: &TestCodex,
    environment_requests: Vec<TurnEnvironmentRequest>,
    approval: AskForApproval,
    profile: PermissionProfile,
) -> Result<()> {
    let (sandbox_policy, permission_profile) = turn_permission_fields(profile, test.cwd_path());
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "perform the exact edit".to_owned(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                environments: Some(TurnEnvironmentRequests::new(
                    test.config.cwd.clone(),
                    environment_requests,
                )),
                approval_policy: Some(approval),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                ..Default::default()
            }),
        )
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_remote_rejects_cardinality_and_non_text_targets_without_writes()
-> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let (builder, outcomes) = ToolOutcomes::observe(structured_edit_builder());
    let test = builder
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let local_path = test.workspace_path("same.txt");
    std::fs::write(&local_path, "local sentinel")?;
    let cases = [
        ("zero.txt", Some(b"before".as_slice()), "absent"),
        ("multiple.txt", Some(b"before before".as_slice()), "before"),
        ("empty.txt", Some(b"before".as_slice()), ""),
        ("invalid.txt", Some(b"\xffbefore".as_slice()), "before"),
        ("missing.txt", None, "before"),
        ("directory", None, "before"),
    ];
    for (name, bytes, old) in cases {
        let path = remote_root.path().join(name);
        if let Some(bytes) = bytes {
            std::fs::write(&path, bytes)?;
        }
        if name == "directory" {
            std::fs::create_dir(&path)?;
        }
        fixture.observe_path(&PathUri::from_host_native_path(&path)?);
        let response = mount_edit(&server, responses::ev_function_call(name, "structured_edit", &json!({"file_path":name,"old_string":old,"new_string":"after","environment_id":"remote"}).to_string())).await;
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
        assert_no_commit(&events);
        outcomes.assert_edit_failed(&events, name);
        let requests = response.requests();
        assert_function_result(output_request(&requests, name), name, false);
        match bytes {
            Some(bytes) => assert_eq!(std::fs::read(&path)?, bytes),
            None if name == "directory" => assert!(path.is_dir()),
            None => assert!(!path.exists()),
        }
    }
    fixture.assert_ready();
    assert!(
        !fixture
            .records()
            .iter()
            .any(|rpc| rpc.method.contains("writeFile"))
    );
    assert_eq!(std::fs::read_to_string(&local_path)?, "local sentinel");
    let remote_path = remote_root.path().join("same.txt");
    let remote_uri = PathUri::from_host_native_path(&remote_path)?;
    std::fs::write(&remote_path, "α\r\nα\r\ntail")?;
    establish_diff_root(&test, REMOTE_ENVIRONMENT_ID, remote_root.path()).await?;
    fixture.observe_path(&remote_uri);
    let response = mount_edit(&server, responses::ev_function_call("replace-all", "structured_edit", &json!({"file_path":"same.txt","old_string":"α","new_string":"β","replace_all":true,"environment_id":"remote"}).to_string())).await;
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
    assert_committed(&finish_turn(&test).await, &remote_path, "remote");
    output_request(&response.requests(), "replace-all");
    assert_eq!(std::fs::read(&remote_path)?, "β\r\nβ\r\ntail".as_bytes());
    assert_eq!(std::fs::read_to_string(&local_path)?, "local sentinel");
    assert_remote_cas(&fixture, &remote_uri);
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}

pub(super) fn assert_committed(events: &[EventMsg], target: &Path, environment: &str) {
    let completed_turn = events
        .iter()
        .find_map(|event| match event {
            EventMsg::TurnComplete(completed) => Some(completed.turn_id.as_str()),
            _ => None,
        })
        .expect("terminal turn identity");
    let ends = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::PatchApplyEnd(end) => Some(end),
            _ => None,
        })
        .collect::<Vec<_>>();
    let committed = ends.first().expect("committed edit must report its result");
    assert!(!committed.call_id.is_empty());
    let Some(FileChange::Update {
        unified_diff: edit, ..
    }) = committed.changes.get(target)
    else {
        panic!("committed edit must identify the selected target");
    };
    for end in &ends {
        assert_eq!(end.call_id, committed.call_id, "only one logical edit");
        assert_eq!(end.turn_id, completed_turn);
        assert!(end.success);
        assert_eq!(end.status, PatchApplyStatus::Completed);
        assert_eq!(end.changes.len(), 1);
        let Some(FileChange::Update {
            unified_diff,
            move_path,
        }) = end.changes.get(target)
        else {
            panic!("completed edit must update only the selected target");
        };
        assert!(move_path.is_none());
        assert_edit_changes(unified_diff, edit);
    }
    let items = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemCompleted(event) => match &event.item {
                TurnItem::FileChange(item) => Some((event.turn_id.as_str(), item)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !items.is_empty(),
        "committed edit must have a public result"
    );
    for (turn_id, item) in items {
        assert_eq!(turn_id, completed_turn);
        assert_eq!(item.id, committed.call_id);
        assert_eq!(item.status, Some(PatchApplyStatus::Completed));
        assert_eq!(item.changes.len(), 1);
        let Some(FileChange::Update {
            unified_diff,
            move_path,
        }) = item.changes.get(target)
        else {
            panic!("public result must update only the selected target");
        };
        assert!(move_path.is_none());
        assert_edit_changes(unified_diff, edit);
    }
    let diff = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::TurnDiff(diff) => Some(diff.unified_diff.as_str()),
            _ => None,
        })
        .next_back()
        .expect("committed edit must report a turn diff");
    assert_edit_diff(diff, target, environment, edit);
}

fn restricted_to(root: &Path) -> Result<PermissionProfile> {
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            FileSystemPath::Special {
                value: FileSystemSpecialPath::Root,
            },
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(
            FileSystemPath::Path {
                path: AbsolutePathBuf::try_from(root.to_path_buf())?.into(),
            },
            FileSystemAccessMode::Write,
        ),
    ]);
    Ok(PermissionProfile::from_runtime_permissions(
        &policy,
        NetworkSandboxPolicy::Restricted,
    ))
}

pub(super) fn assert_remote_cas(fixture: &RemoteFixture, target: &PathUri) {
    fixture.assert_ready();
    let records = fixture.records();
    assert!(
        records.iter().any(|rpc| rpc.method == "fs/readFile"
            && rpc.path == target.to_string()
            && rpc.terminal == Some(RpcTerminal::Success)),
        "planning must read the selected Remote target"
    );
    let writes = records
        .iter()
        .filter(|rpc| rpc.method == "fs/writeFileIfUnchanged")
        .collect::<Vec<_>>();
    assert!(!writes.is_empty(), "conditional write must reach Remote");
    assert!(
        writes.iter().all(|rpc| rpc.path == target.to_string()),
        "no write may target another file"
    );
    assert!(
        writes
            .iter()
            .any(|rpc| rpc.terminal == Some(RpcTerminal::Written)),
        "Remote must confirm a conditional commit"
    );
    assert!(!records.iter().any(|rpc| rpc.method == "fs/writeFile"));
}

#[test_case("local", "relative"; "local_relative")]
#[test_case("remote", "relative"; "remote_relative")]
#[test_case("remote", "absolute"; "remote_absolute")]
#[test_case("remote", "primary"; "remote_primary_without_argument")]
#[test_case("local", "restricted"; "local_actual_restricted_helper")]
#[test_case("remote", "restricted"; "remote_actual_restricted_helper")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_routes_exact_bytes_to_selected_environment(
    environment: &str,
    path_mode: &str,
) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let local_helper = fixture::ObservedLocalHelper::new()?;
    let server = responses::start_mock_server().await;
    let test = structured_edit_builder()
        .with_exec_server_url(fixture.url())
        .with_local_runtime_paths(local_helper.runtime_paths()?)
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let local_path = test.workspace_path("same.txt");
    let remote_path = remote_root.path().join("same.txt");
    std::fs::write(&local_path, b"LOCAL\r\nkeep")?;
    std::fs::write(&remote_path, b"REMOTE\r\nkeep")?;
    let target = if environment == REMOTE_ENVIRONMENT_ID {
        &remote_path
    } else {
        &local_path
    };
    let target_uri = PathUri::from_host_native_path(target)?;
    establish_diff_root(&test, environment, target.parent().context("selected cwd")?).await?;
    fixture.observe_path(&PathUri::from_host_native_path(&local_path)?);
    fixture.observe_path(&PathUri::from_host_native_path(&remote_path)?);
    let remote = test
        .thread_manager
        .environment_manager()
        .get_environment(REMOTE_ENVIRONMENT_ID)
        .context("registered Remote")?;
    assert!(remote.is_remote());
    remote.wait_until_ready().await?;
    fixture.assert_ready();
    let helper_calls_before_edit = local_helper.invocation_count()?;
    let mut args = json!({"file_path": if path_mode == "absolute" { target.to_str().context("UTF-8 fixture path")? } else { "same.txt" }, "old_string": environment.to_uppercase(), "new_string": "changed"});
    if path_mode != "primary" {
        args["environment_id"] = json!(environment);
    }
    let response = mount_edit(
        &server,
        responses::ev_function_call("edit", "structured_edit", &args.to_string()),
    )
    .await;
    let mut selections = vec![
        environment_request(LOCAL_ENVIRONMENT_ID, test.cwd_path())?,
        environment_request(REMOTE_ENVIRONMENT_ID, remote_root.path())?,
    ];
    if path_mode == "primary" {
        selections.reverse();
    }
    let profile = if path_mode == "restricted" {
        restricted_to(target.parent().context("target root")?)?
    } else {
        PermissionProfile::Disabled
    };
    start_turn(&test, selections, AskForApproval::Never, profile).await?;
    let events = finish_turn(&test).await;
    assert_committed(&events, target, environment);
    let requests = response.requests();
    let (_, success) = output_request(&requests, "edit")
        .function_call_output_content_and_success("edit")
        .context("paired edit output")?;
    assert_ne!(success, Some(false));
    assert_eq!(
        std::fs::read(&local_path)?,
        if environment == LOCAL_ENVIRONMENT_ID {
            b"changed\r\nkeep".as_slice()
        } else {
            b"LOCAL\r\nkeep".as_slice()
        }
    );
    assert_eq!(
        std::fs::read(&remote_path)?,
        if environment == REMOTE_ENVIRONMENT_ID {
            b"changed\r\nkeep".as_slice()
        } else {
            b"REMOTE\r\nkeep".as_slice()
        }
    );
    if environment == REMOTE_ENVIRONMENT_ID {
        assert_remote_cas(&fixture, &target_uri);
    } else {
        assert!(
            !fixture
                .records()
                .iter()
                .any(|rpc| rpc.method.contains("writeFile"))
        );
    }
    if environment == REMOTE_ENVIRONMENT_ID && path_mode == "restricted" {
        assert!(
            fixture
                .records()
                .iter()
                .filter(|rpc| rpc.method == "fs/writeFileIfUnchanged")
                .all(|rpc| rpc.sandboxed)
        );
    }
    let helper_calls = local_helper.invocation_count()? - helper_calls_before_edit;
    if environment == LOCAL_ENVIRONMENT_ID && path_mode == "restricted" {
        assert!(
            helper_calls > 0,
            "restricted Local write must reach its real helper"
        );
    } else {
        assert_eq!(
            helper_calls, 0,
            "other routes must not launch the Local sandbox writer"
        );
    }
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_unavailable_remote_never_falls_back_to_usable_local() -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let (builder, outcomes) = ToolOutcomes::observe(structured_edit_builder());
    let test = builder
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let local_path = test.workspace_path("same.txt");
    let remote_path = remote_root.path().join("same.txt");
    std::fs::write(&local_path, "before")?;
    std::fs::write(&remote_path, "before")?;
    test.thread_manager
        .environment_manager()
        .get_environment(REMOTE_ENVIRONMENT_ID)
        .context("Remote")?
        .wait_until_ready()
        .await?;
    fixture.assert_ready();
    // Replace the real connected backend by its now-closed endpoint and a bounded
    // connection timeout. Local remains registered and usable throughout.
    let unavailable_url = fixture.url().to_owned();
    fixture.stop().await?;
    test.thread_manager
        .environment_manager()
        .upsert_environment(
            REMOTE_ENVIRONMENT_ID.to_owned(),
            unavailable_url,
            Some(Duration::from_millis(300)),
        )?;
    let response = mount_edit(&server, responses::ev_function_call("unavailable", "structured_edit", &json!({"file_path":"same.txt","old_string":"before","new_string":"wrong","environment_id":REMOTE_ENVIRONMENT_ID}).to_string())).await;
    let selections = vec![
        environment_request(LOCAL_ENVIRONMENT_ID, test.cwd_path())?,
        environment_request(REMOTE_ENVIRONMENT_ID, remote_root.path())?,
    ];
    start_turn(
        &test,
        selections.clone(),
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(&test).await;
    assert_no_commit(&events);
    outcomes.assert_edit_failed(&events, "unavailable");
    let requests = response.requests();
    let (output, success) = output_request(&requests, "unavailable")
        .function_call_output_content_and_success("unavailable")
        .context("Remote failure output")?;
    assert!(
        output
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty()),
        "unavailable Remote must return a diagnostic"
    );
    assert_ne!(
        success,
        Some(true),
        "unavailable Remote must not report success; output={output:?}"
    );
    assert_eq!(
        (std::fs::read(&local_path)?, std::fs::read(&remote_path)?),
        (b"before".to_vec(), b"before".to_vec())
    );
    let selected = test.codex.environment_selections().await;
    assert_eq!(
        selected
            .iter()
            .map(|selection| selection.environment_id.as_str())
            .collect::<Vec<_>>(),
        vec![LOCAL_ENVIRONMENT_ID, REMOTE_ENVIRONMENT_ID]
    );
    let manager = test.thread_manager.environment_manager();
    assert_eq!(
        manager.get_environment_status(LOCAL_ENVIRONMENT_ID).await,
        Some(EnvironmentObservedStatus::Ready)
    );
    assert!(matches!(
        manager.get_environment_status(REMOTE_ENVIRONMENT_ID).await,
        Some(EnvironmentObservedStatus::Disconnected { .. })
    ));
    establish_diff_root(&test, LOCAL_ENVIRONMENT_ID, test.cwd_path()).await?;
    // A separate default-routed Local edit must remain usable after Remote fails.
    let local_response = mount_edit(&server, responses::ev_function_call("local", "structured_edit", &json!({"file_path":"same.txt","old_string":"before","new_string":"local remains usable"}).to_string())).await;
    start_turn(
        &test,
        selections,
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;
    let events = finish_turn(&test).await;
    let local_requests = local_response.requests();
    assert_function_result(output_request(&local_requests, "local"), "local", true);
    assert_committed(&events, &local_path, LOCAL_ENVIRONMENT_ID);
    assert_eq!(
        std::fs::read_to_string(&local_path)?,
        "local remains usable"
    );
    assert_eq!(std::fs::read_to_string(&remote_path)?, "before");
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[test_case("replace"; "stale_snapshot")]
#[test_case("remove"; "read_failure")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_remote_target_changes_after_approval_have_no_cas_or_committed_diff(
    mutation: &str,
) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let test = structured_edit_builder()
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    let path = remote_root.path().join("same.txt");
    std::fs::write(&path, "before")?;
    fixture.observe_path(&PathUri::from_host_native_path(&path)?);
    let response = mount_edit(&server, responses::ev_function_call("stale", "structured_edit", &json!({"file_path":"same.txt","old_string":"before","new_string":"wrong","environment_id":REMOTE_ENVIRONMENT_ID}).to_string())).await;
    start_turn(
        &test,
        vec![
            environment_request(LOCAL_ENVIRONMENT_ID, test.cwd_path())?,
            environment_request(REMOTE_ENVIRONMENT_ID, remote_root.path())?,
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
    if mutation == "replace" {
        std::fs::write(&path, "competing bytes")?;
    } else {
        std::fs::remove_file(&path)?;
    }
    test.codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Approved,
        })
        .await?;
    let events = finish_turn(&test).await;
    assert_no_commit(&events);
    let requests = response.requests();
    let (output, _) = output_request(&requests, "stale")
        .function_call_output_content_and_success("stale")
        .context("stale output")?;
    let expected = if mutation == "replace" {
        codex_apply_patch::STALE_STRUCTURED_EDIT_MESSAGE
    } else {
        "Failed to read file"
    };
    assert!(output.context("edit error text")?.contains(expected));
    if mutation == "replace" {
        assert_eq!(std::fs::read_to_string(&path)?, "competing bytes");
    } else {
        assert!(!path.exists());
    }
    fixture.assert_ready();
    assert!(
        !fixture
            .records()
            .iter()
            .any(|rpc| rpc.method.contains("writeFile"))
    );
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}

#[test_case("local"; "local")]
#[test_case("remote"; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_verified_writer_reaches_real_sandbox_allow_and_deny(
    environment: &str,
) -> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let allowed_root = TempDir::new()?;
    let denied_root = TempDir::new()?;
    let allowed = allowed_root.path().join("same.txt");
    let denied = denied_root.path().join("same.txt");
    std::fs::write(&allowed, "before")?;
    std::fs::write(&denied, "before")?;
    let fs: Arc<dyn ExecutorFileSystem> = if environment == REMOTE_ENVIRONMENT_ID {
        let remote = Environment::create_for_tests(Some(fixture.url().to_owned()))?;
        assert!(remote.is_remote());
        remote.wait_until_ready().await?;
        remote.get_filesystem()
    } else {
        Arc::new(LocalFileSystem::with_runtime_paths(
            ExecServerRuntimeOptions::new(
                fixture::editor_binary("codex")?,
                Some(fixture::editor_binary("codex-linux-sandbox")?),
            )?,
        ))
    };
    let cwd = PathUri::from_host_native_path(allowed_root.path())?;
    let sandbox = FileSystemSandboxContext::from_permission_profile(
        restricted_to(allowed_root.path())?,
        cwd.clone(),
    );
    for (path, permitted) in [(&allowed, true), (&denied, false)] {
        let path_uri = PathUri::from_host_native_path(path)?;
        fixture.observe_path(&path_uri);
        let action = ApplyPatchAction::from_exact_update(
            cwd.clone(),
            path_uri,
            "before",
            "after".to_owned(),
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = apply_verified_action(
            &action,
            ApplyPatchOptions::default(),
            &mut stdout,
            &mut stderr,
            fs.as_ref(),
            Some(&sandbox),
        )
        .await;
        if permitted {
            let delta = result.context(
                "required real sandbox allow failed: helper/namespace prerequisites must work",
            )?;
            assert!(delta.is_exact());
            assert_eq!(delta.changes().len(), 1);
            assert_eq!(std::fs::read_to_string(path)?, "after");
        } else {
            let failure = result.expect_err("real sandbox must deny sibling-root write");
            let error = failure.to_string();
            assert!(
                String::from_utf8(stderr)?.starts_with("Failed to write file "),
                "denial must reach the writer after its successful final read"
            );
            assert!(
                stdout.is_empty(),
                "denied write must have no success summary"
            );
            assert!(
                !error.contains("fs sandbox helper failed")
                    && !error.contains("namespace")
                    && !error.contains("bwrap:"),
                "helper startup failure is a prerequisite failure, not a policy denial: {error}"
            );
            assert!(
                error.contains("Permission denied")
                    || error.contains("Read-only file system")
                    || error.contains("Operation not permitted"),
                "policy-specific write failure required: {error}"
            );
            assert!(failure.delta().changes().is_empty());
            assert!(!failure.delta().is_exact());
            assert_eq!(std::fs::read_to_string(path)?, "before");
        }
    }
    if environment == REMOTE_ENVIRONMENT_ID {
        fixture.assert_ready();
        let records = fixture.records();
        let writes: Vec<_> = records
            .iter()
            .filter(|rpc| rpc.method == "fs/writeFileIfUnchanged")
            .collect();
        assert!(writes.iter().all(|rpc| rpc.sandboxed));
        for (path, terminal) in [
            (&allowed, RpcTerminal::Written),
            (&denied, RpcTerminal::FileAccessDenied),
        ] {
            let path = PathUri::from_host_native_path(path)?.to_string();
            assert!(
                writes
                    .iter()
                    .any(|rpc| rpc.path == path && rpc.terminal.as_ref() == Some(&terminal)),
                "actual writer must report the bound allow/deny outcome for {path}"
            );
        }
        let allowed_uri = PathUri::from_host_native_path(&allowed)?.to_string();
        let denied_uri = PathUri::from_host_native_path(&denied)?.to_string();
        assert!(
            writes.iter().all(|rpc| {
                if rpc.path == allowed_uri {
                    matches!(
                        rpc.terminal,
                        Some(RpcTerminal::Written | RpcTerminal::Conflict)
                    )
                } else {
                    rpc.path == denied_uri && rpc.terminal == Some(RpcTerminal::FileAccessDenied)
                }
            }),
            "writer outcomes must agree with each target's permission boundary"
        );
    }
    fixture.stop().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_edit_remote_lost_cas_reply_never_retries_or_reports_a_known_commit()
-> Result<()> {
    let mut fixture = RemoteFixture::start().await?;
    let server = responses::start_mock_server().await;
    let test = structured_edit_builder()
        .with_exec_server_url(fixture.url())
        .build_with_remote_and_local_env(&server)
        .await?;
    let remote_root = TempDir::new()?;
    // Include "sandbox" in the path to defeat message-substring denial
    // heuristics: a transport-uncertain CAS must never become a retry prompt.
    let path = remote_root.path().join("sandbox-lost-reply.txt");
    let path_uri = PathUri::from_host_native_path(&path)?;
    std::fs::write(&path, "before")?;
    fixture.observe_path(&path_uri);
    fixture.drop_written_reply_once(&path_uri);
    let response = mount_edit(&server, responses::ev_function_call("lost-reply", "structured_edit", &json!({"file_path":"sandbox-lost-reply.txt","old_string":"before","new_string":"actually committed","environment_id":"remote"}).to_string())).await;
    start_turn(
        &test,
        vec![
            environment_request("local", test.cwd_path())?,
            environment_request("remote", remote_root.path())?,
        ],
        AskForApproval::OnRequest,
        restricted_to(remote_root.path())?,
    )
    .await?;
    // finish_turn fails immediately if any retry approval is emitted.
    let events = finish_turn(&test).await;
    fixture.assert_written_reply_dropped();
    fixture.assert_ready();
    let records = fixture.records();
    let writes: Vec<_> = records
        .iter()
        .filter(|rpc| rpc.method == "fs/writeFileIfUnchanged")
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "all proxy connections share one mutation count"
    );
    assert!(writes[0].sandboxed, "real restricted writer must have run");
    assert_eq!(writes[0].terminal, Some(RpcTerminal::Written));
    assert!(!records.iter().any(|rpc| rpc.method == "fs/writeFile"));
    assert_eq!(std::fs::read_to_string(&path)?, "actually committed");
    let requests = response.requests();
    let text = output_request(&requests, "lost-reply")
        .function_call_output_text("lost-reply")
        .context("lost-reply terminal error")?;
    assert!(text.contains("Failed to write file"), "{text}");
    assert!(!text.contains("Success. Updated"), "{text}");
    assert_no_commit(&events);
    assert!(events.iter().any(|event| matches!(event, EventMsg::PatchApplyEnd(end) if end.status == PatchApplyStatus::Failed)));
    test.codex.shutdown_and_wait().await?;
    fixture.stop().await
}
