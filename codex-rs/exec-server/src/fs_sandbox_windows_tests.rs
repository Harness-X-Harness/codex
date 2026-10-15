#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
use codex_protocol::config_types::WindowsSandboxLevel;
#[cfg(windows)]
use codex_protocol::models::PermissionProfile;
#[cfg(windows)]
use codex_sandboxing::SandboxExecRequest;
#[cfg(windows)]
use codex_sandboxing::SandboxType;
#[cfg(windows)]
use codex_utils_path_uri::PathUri;
use codex_utils_pty::Child;
use codex_utils_pty::ChildStdin;
use codex_utils_pty::Command;
use pretty_assertions::assert_eq;
#[cfg(windows)]
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use super::drain_helper_stderr;
use super::read_helper_response;
use super::reap_helper_after_response;
#[cfg(windows)]
use super::run_command;
#[cfg(windows)]
use crate::fs_helper::FsHelperPayload;
#[cfg(windows)]
use crate::fs_helper::FsHelperRequest;
#[cfg(windows)]
use crate::protocol::FsReadFileParams;
#[cfg(windows)]
use crate::protocol::FsReadFileResponse;
#[cfg(windows)]
use crate::protocol::FsWriteFileParams;
#[cfg(windows)]
use crate::protocol::FsWriteFileResponse;

#[tokio::test(start_paused = true)]
async fn filesystem_operation_is_not_limited_by_helper_response_deadline() {
    let (reader, mut writer) = tokio::io::duplex(/*max_buf_size*/ 256);
    let response = tokio::spawn(async move { read_helper_response(reader).await });

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 31)).await;
    tokio::task::yield_now().await;
    assert!(
        !response.is_finished(),
        "a filesystem operation must not time out before the helper responds"
    );

    writer
        .write_all(b"completed after the old deadline\n")
        .await
        .expect("helper response");

    assert_eq!(
        response
            .await
            .expect("response task")
            .expect("unbounded filesystem operation"),
        b"completed after the old deadline\n"
    );
}

#[tokio::test]
async fn noisy_failing_helper_preserves_exit_status_and_bounded_stderr() {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg("-c").arg(
            "printf 'ready\\n'; read -r ready; [ \"$ready\" = go ] || exit 9; printf 'expected helper diagnostic' >&2; i=0; while [ \"$i\" -lt 1024 ]; do printf '%0128d' 0 >&2; i=$((i + 1)); done; exit 7",
        );
        command
    };
    #[cfg(windows)]
    let mut command = {
        let system_root = std::env::var("SystemRoot").expect("Windows system root");
        let powershell = Path::new(&system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let mut command = Command::new(powershell);
        command
            .arg("-NoProfile")
            .arg("-Command")
            .arg("[Console]::Out.WriteLine('ready'); [Console]::Out.Flush(); if ([Console]::In.ReadLine() -ne 'go') { exit 9 }; [Console]::Error.Write('expected helper diagnostic' + ('x' * 131072)); exit 7");
        command
    };
    command.envs(std::env::vars_os());
    command.stdin(ChildStdin::Piped);
    let mut child = command.spawn().expect("noisy helper process");
    let stderr = drain_helper_stderr(&mut child);
    release_started_helper(&mut child).await;

    let error = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 8),
        reap_helper_after_response(child, stderr),
    )
    .await
    .expect("helper stderr must be drained during bounded cleanup")
    .expect_err("nonzero helper exit should fail after its stderr pipe fills");

    assert!(error.message.contains('7'), "{}", error.message);
    assert!(
        error.message.contains("expected helper diagnostic"),
        "{}",
        error.message
    );
    assert!(error.message.len() < 4400, "{}", error.message.len());
}

#[tokio::test]
async fn helper_stderr_is_drained_before_the_response() {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg("-c").arg(
            "printf 'ready\\n'; read -r ready; [ \"$ready\" = go ] || exit 9; printf 'expected pre-response diagnostic' >&2; i=0; while [ \"$i\" -lt 1024 ]; do printf '%0128d' 0 >&2; i=$((i + 1)); done; printf 'completed after noisy stderr\\n'",
        );
        command
    };
    #[cfg(windows)]
    let mut command = {
        let system_root = std::env::var("SystemRoot").expect("Windows system root");
        let powershell = Path::new(&system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let mut command = Command::new(powershell);
        command.arg("-NoProfile").arg("-Command").arg(
            "[Console]::Out.WriteLine('ready'); [Console]::Out.Flush(); if ([Console]::In.ReadLine() -ne 'go') { exit 9 }; [Console]::Error.Write('expected pre-response diagnostic' + ('x' * 131072)); [Console]::Out.WriteLine('completed after noisy stderr')",
        );
        command
    };
    command.envs(std::env::vars_os());
    command.stdin(ChildStdin::Piped);
    let mut child = command.spawn().expect("noisy helper process");
    let stderr = drain_helper_stderr(&mut child);
    release_started_helper(&mut child).await;
    let stdout = child.stdout.take().expect("helper stdout");

    let response = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 2),
        read_helper_response(stdout),
    )
    .await
    .expect("helper stderr must be drained before awaiting its response")
    .expect("helper response");

    assert_eq!(response.trim_ascii_end(), b"completed after noisy stderr");
    reap_helper_after_response(child, stderr)
        .await
        .expect("noisy helper should be cleaned up after its response");
}

// Bound interpreter startup separately; the response/cleanup assertions below still cover
// the actual pipe-filling work and keep their original deadlines.
async fn release_started_helper(child: &mut Child) {
    let ready = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 10),
        read_helper_response(child.stdout.as_mut().expect("helper stdout")),
    )
    .await
    .expect("helper interpreter starts")
    .expect("helper readiness response");
    assert_eq!(ready.trim_ascii_end(), b"ready");
    child
        .stdin
        .as_mut()
        .expect("helper stdin")
        .write_all(b"go\n")
        .await
        .expect("release actual noisy helper work");
}

#[cfg(windows)]
#[tokio::test]
async fn completed_windows_image_read_does_not_wait_for_a_stuck_helper() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("image.png");
    let operations = [
        (
            r#"[IO.File]::WriteAllText($env:CODEX_FS_HELPER_TEST_PATH, 'image')
[Console]::Out.WriteLine('{"status":"ok","payload":{"operation":"fs/writeFile","response":{}}}')"#,
            FsHelperPayload::WriteFile(FsWriteFileResponse {}),
        ),
        (
            r#"$data = [Convert]::ToBase64String([IO.File]::ReadAllBytes($env:CODEX_FS_HELPER_TEST_PATH))
[Console]::Out.WriteLine('{"status":"ok","payload":{"operation":"fs/readFile","response":{"dataBase64":"' + $data + '"}}}')"#,
            FsHelperPayload::ReadFile(FsReadFileResponse {
                data_base64: "aW1hZ2U=".to_string(),
            }),
        ),
    ];

    for (operation, expected) in operations {
        let script = format!(
            "[Console]::In.ReadLine() | Out-Null\n{operation}\n[Console]::Out.Flush()\n[Threading.Thread]::Sleep(30000)"
        );
        let command = powershell_command(&script, &path).expect("PowerShell helper command");
        let request = if matches!(&expected, FsHelperPayload::WriteFile(_)) {
            FsHelperRequest::WriteFile(FsWriteFileParams {
                path: PathUri::from_host_native_path(&path).expect("write URI"),
                data_base64: "aW1hZ2U=".to_string(),
                follow_symlinks: None,
                sandbox: None,
            })
        } else {
            FsHelperRequest::ReadFile(FsReadFileParams {
                path: PathUri::from_host_native_path(&path).expect("read URI"),
                follow_symlinks: None,
                sandbox: None,
            })
        };
        let result = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 8),
            run_command(command, request),
        )
        .await
        .expect("the completed operation must not wait for helper termination")
        .expect("helper response");

        assert_eq!(result, expected);
    }
    assert_eq!(std::fs::read(&path).expect("created file"), b"image");
    let file_system: std::sync::Arc<dyn crate::ExecutorFileSystem> =
        std::sync::Arc::new(crate::LocalFileSystem::unsandboxed());
    assert_eq!(
        file_system
            .write_file_if_unchanged(
                &PathUri::from_host_native_path(&path).expect("path URI"),
                b"image".to_vec(),
                b"updated".to_vec(),
                crate::WriteFileOptions::default(),
                /*sandbox*/ None,
            )
            .await
            .expect("validated terminal response leaves mutation admission open"),
        crate::ConditionalWriteResult::Written
    );
}

#[cfg(windows)]
#[tokio::test]
async fn duplicated_windows_file_handle_survives_bounded_helper_cleanup() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("image.png");
    std::fs::write(&path, b"image").expect("image file");
    let command = powershell_command(
        r#"[Console]::In.ReadLine() | Out-Null
$file = [IO.File]::OpenRead($env:CODEX_FS_HELPER_TEST_PATH)
$handle = $file.SafeFileHandle.DangerousGetHandle().ToInt64()
[Console]::Out.WriteLine('{"status":"ok","payload":{"operation":"fs/open","response":{"processId":' + $PID + ',"fileHandle":' + $handle + '}}}')
[Console]::Out.Flush()
[Threading.Thread]::Sleep(30000)"#,
        &path,
    )
    .expect("PowerShell helper command");

    let mut file = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 8),
        crate::sandboxed_file_open::open(
            command,
            PathUri::from_host_native_path(&path).expect("image path URI"),
        ),
    )
    .await
    .expect("the opened file must not wait for helper termination")
    .expect("duplicated file handle");
    let mut data = Vec::new();
    file.read_to_end(&mut data).await.expect("image contents");

    assert_eq!(data, b"image");
}

#[cfg(windows)]
fn powershell_command(script: &str, path: &Path) -> anyhow::Result<SandboxExecRequest> {
    let system_root = std::env::var("SystemRoot")?;
    let powershell = Path::new(&system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let cwd = PathUri::from_host_native_path(std::env::current_dir()?)?;

    Ok(SandboxExecRequest {
        command: vec![
            powershell.to_string_lossy().into_owned(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            script.to_string(),
        ],
        cwd: cwd.clone(),
        sandbox_policy_cwd: cwd,
        env: HashMap::from([
            ("SystemRoot".to_string(), system_root),
            (
                "CODEX_FS_HELPER_TEST_PATH".to_string(),
                path.to_string_lossy().into_owned(),
            ),
        ]),
        network: None,
        network_environment_id: None,
        sandbox: SandboxType::None,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
        permission_profile: PermissionProfile::Disabled,
        arg0: None,
    })
}

#[cfg(windows)]
#[tokio::test]
async fn unconfirmed_windows_helper_effect_closes_mutation_admission() {
    const CHILD_ENV: &str = "CODEX_FS_UNKNOWN_EFFECT_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        // The deliberately closed process-wide owner must not contaminate other unit tests.
        let output = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 20),
            tokio::process::Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "fs_sandbox::windows_tests::unconfirmed_windows_helper_effect_closes_mutation_admission", "--nocapture"])
                .env(CHILD_ENV, "1")
                .kill_on_drop(true)
                .output(),
        ).await.expect("isolated uncertainty fixture completes").expect("isolated test process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"),
            "the isolated helper test must execute exactly once: {stdout}"
        );
        assert!(
            output.status.success(),
            "isolated unknown-effect test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    std::fs::write(&path, b"before").expect("initial bytes");
    let command = powershell_command(
        "[Console]::In.ReadLine() | Out-Null\n[Console]::Out.WriteLine('invalid response')\n[Console]::Out.Flush()\n[Threading.Thread]::Sleep(30000)",
        &path,
    ).expect("helper command");
    run_command(
        command,
        FsHelperRequest::WriteFile(FsWriteFileParams {
            path: PathUri::from_host_native_path(&path).expect("path URI"),
            data_base64: "YWZ0ZXI=".to_string(),
            follow_symlinks: None,
            sandbox: None,
        }),
    )
    .await
    .expect_err("invalid response without observed normal exit is uncertain");
    let file_system: std::sync::Arc<dyn crate::ExecutorFileSystem> =
        std::sync::Arc::new(crate::LocalFileSystem::unsandboxed());
    let error = file_system
        .write_file_if_unchanged(
            &PathUri::from_host_native_path(&path).expect("path URI"),
            b"before".to_vec(),
            b"after".to_vec(),
            crate::WriteFileOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .expect_err("admission stays closed after unconfirmed effect");
    assert!(error.to_string().contains("admission closed"), "{error}");
    assert_eq!(std::fs::read(path).expect("unchanged bytes"), b"before");
}

#[cfg(windows)]
#[tokio::test]
async fn terminal_windows_helper_error_settles_partial_effect_before_slow_exit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    let command = powershell_command(
        r#"[Console]::In.ReadLine() | Out-Null
[IO.File]::WriteAllText($env:CODEX_FS_HELPER_TEST_PATH, 'partial')
[Console]::Out.WriteLine('{"status":"error","payload":{"code":-32602,"message":"controlled settled error","data":null}}')
[Console]::Out.Flush()
[Threading.Thread]::Sleep(30000)"#,
        &path,
    ).expect("helper command");
    let error = run_command(
        command,
        FsHelperRequest::WriteFile(FsWriteFileParams {
            path: PathUri::from_host_native_path(&path).expect("path URI"),
            data_base64: "cGFydGlhbA==".to_string(),
            follow_symlinks: None,
            sandbox: None,
        }),
    )
    .await
    .expect_err("terminal helper error is preserved");
    assert_eq!(error.message, "controlled settled error");
    let file_system: std::sync::Arc<dyn crate::ExecutorFileSystem> =
        std::sync::Arc::new(crate::LocalFileSystem::unsandboxed());
    assert_eq!(
        file_system
            .write_file_if_unchanged(
                &PathUri::from_host_native_path(&path).expect("path URI"),
                b"partial".to_vec(),
                b"after".to_vec(),
                crate::WriteFileOptions::default(),
                /*sandbox*/ None,
            )
            .await
            .expect("settled error releases admission"),
        crate::ConditionalWriteResult::Written
    );
    assert_eq!(std::fs::read(path).expect("successor bytes"), b"after");
}
