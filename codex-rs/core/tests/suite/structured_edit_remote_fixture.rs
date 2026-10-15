//! A real same-host executor, with bounded RPC observation and an explicit lost-reply fault.
//! Separate temporary directories are routing controls, not filesystem namespaces.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Counts actual Local sandbox-helper launches, then transparently execs the
/// pinned helper. No filesystem operation, argument, or response is simulated.
pub(crate) struct ObservedLocalHelper {
    directory: TempDir,
}

impl ObservedLocalHelper {
    pub fn new() -> Result<Self> {
        let directory = TempDir::new()?;
        let helper = editor_binary("codex-linux-sandbox")?;
        let marker = directory.path().join("invocations");
        let script = directory.path().join("codex-linux-sandbox");
        let marker = shlex::try_join([marker.to_str().context("helper marker path")?])?;
        let helper = shlex::try_join([helper.to_str().context("bound Linux helper path")?])?;
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf 'sandbox\\n' >> {marker}\nexec {helper} \"$@\"\n"),
        )?;
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))?;
        Ok(Self { directory })
    }

    pub fn runtime_paths(&self) -> Result<codex_exec_server::ExecServerRuntimePaths> {
        Ok(codex_exec_server::ExecServerRuntimePaths::new(
            editor_binary("codex")?,
            Some(self.directory.path().join("codex-linux-sandbox")),
        )?)
    }

    pub fn invocation_count(&self) -> Result<usize> {
        match std::fs::read_to_string(self.directory.path().join("invocations")) {
            Ok(text) => {
                ensure!(text.len() <= 512, "bounded helper observation exceeded");
                ensure!(
                    text.lines().all(|line| line == "sandbox"),
                    "invalid helper observation"
                );
                Ok(text.lines().count())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error.into()),
        }
    }
}

/// Ordinary Cargo/Bazel runs use their standard binary resolver. Required Grok
/// proof runs must supply explicit executable paths and their bound SHA256s.
pub(crate) fn editor_binary(name: &str) -> Result<PathBuf> {
    let variable = match name {
        "codex" => "CODEX_C4B_TEST_CLI",
        "codex-linux-sandbox" => "CODEX_TEST_LINUX_SANDBOX_EXE",
        "codex-code-mode-host" => "CODEX_C4B_CODE_MODE_HOST",
        _ => anyhow::bail!("unknown editor fixture binary {name}"),
    };
    let required = match std::env::var_os("CODEX_C4B_REQUIRED") {
        None => false,
        Some(value) if value == "1" => true,
        Some(_) => anyhow::bail!("CODEX_C4B_REQUIRED must be 1 or unset"),
    };
    let digest_variable = format!("{variable}_SHA256");
    let digest = std::env::var_os(&digest_variable)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("{digest_variable} must be UTF-8"))
        })
        .transpose()?;
    resolve_binary(
        name,
        variable,
        std::env::var_os(variable).map(PathBuf::from),
        digest,
        required,
    )
}

fn resolve_binary(
    name: &str,
    variable: &str,
    binding: Option<PathBuf>,
    digest: Option<String>,
    required: bool,
) -> Result<PathBuf> {
    ensure!(
        !required || binding.is_some(),
        "required Grok proof must bind {variable}"
    );
    ensure!(
        !required || digest.is_some(),
        "required Grok proof must bind {variable}_SHA256"
    );
    ensure!(
        binding.is_some() || digest.is_none(),
        "{variable}_SHA256 requires an explicit {variable}"
    );
    if let Some(digest) = &digest {
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "{variable}_SHA256 must contain exactly 64 lowercase hexadecimal characters"
        );
    }
    // An explicit binding is authoritative even outside required mode. An
    // invalid path or digest never causes fallback to Cargo, Bazel, or PATH.
    let path = match binding {
        Some(path) => path,
        None => codex_utils_cargo_bin::cargo_bin(name)?,
    };
    ensure!(
        path.is_absolute(),
        "{variable} must resolve to an absolute path"
    );
    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("{variable} executable is unavailable"))?;
    ensure!(
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        "{variable} must name an executable file"
    );
    if let Some(expected) = digest {
        let output = std::process::Command::new("sha256sum")
            .stdin(Stdio::from(std::fs::File::open(&path)?))
            .output()
            .context("bound Linux proof requires sha256sum")?;
        ensure!(output.status.success(), "sha256sum failed for {variable}");
        verify_binary_digest(variable, &expected, &output.stdout)?;
    }
    Ok(path)
}

fn verify_binary_digest(variable: &str, expected: &str, output: &[u8]) -> Result<()> {
    let actual = std::str::from_utf8(output)?;
    ensure!(
        actual.split_whitespace().eq([expected, "-"]),
        "{variable} does not match its bound SHA256"
    );
    Ok(())
}

#[test]
fn editor_binary_selection_preserves_standard_resolution_and_explicit_failures() -> Result<()> {
    let standard = codex_utils_cargo_bin::cargo_bin("codex")?;
    assert_eq!(
        resolve_binary(
            "codex", "fixture", /*binding*/ None, /*digest*/ None,
            /*required*/ false
        )?,
        standard
    );
    assert_eq!(
        resolve_binary(
            "unused",
            "fixture",
            Some(standard),
            /*digest*/ None,
            /*required*/ false
        )?,
        codex_utils_cargo_bin::cargo_bin("codex")?
    );
    let directory = TempDir::new()?;
    for (binding, digest, required, reason) in [
        (None, None, true, "must bind fixture"),
        (
            Some(std::env::current_exe()?),
            None,
            true,
            "must bind fixture_SHA256",
        ),
        (
            Some(PathBuf::from("relative")),
            None,
            false,
            "absolute path",
        ),
        (
            Some(directory.path().join("missing")),
            None,
            false,
            "unavailable",
        ),
        (
            Some(directory.path().to_path_buf()),
            None,
            false,
            "executable file",
        ),
        (None, Some("0".repeat(64)), false, "requires an explicit"),
        (
            Some(std::env::current_exe()?),
            Some("invalid".to_owned()),
            false,
            "64 lowercase",
        ),
    ] {
        let error = resolve_binary("must-not-fallback", "fixture", binding, digest, required)
            .expect_err("invalid binding");
        assert!(error.to_string().contains(reason), "{error}");
    }
    let digest = "0".repeat(64);
    verify_binary_digest("fixture", &digest, format!("{digest}  -\n").as_bytes())?;
    for output in [
        format!("{}  -\n", "1".repeat(64)),
        format!("{digest}  unexpected-file\n"),
        format!("{digest}  - extra\n"),
    ] {
        assert!(
            verify_binary_digest("fixture", &digest, output.as_bytes()).is_err(),
            "digest mismatch or malformed utility output must fail without binary fallback"
        );
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedRpc {
    pub method: String,
    pub path: String,
    pub sandboxed: bool,
    pub terminal: Option<RpcTerminal>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RpcTerminal {
    Success,
    Written,
    Conflict,
    FileAccessDenied,
    Error,
}

#[derive(Default)]
struct Observation {
    initialized: bool,
    paths: Vec<String>,
    records: Vec<ObservedRpc>,
    drop_written_reply: Option<String>,
    written_reply_dropped: bool,
}

pub(crate) struct RemoteFixture {
    child: ReapedChild,
    proxy: JoinHandle<Result<()>>,
    observation: Arc<Mutex<Observation>>,
    websocket_url: String,
    _home: TempDir,
    _process_cwd: TempDir,
}

struct ReapedChild(Child);

impl Drop for ReapedChild {
    fn drop(&mut self) {
        // This also owns failures before readiness/fixture construction. Normal
        // completion uses stop() and explicitly awaits the same child first.
        let _ = self.0.start_kill();
        let deadline = std::time::Instant::now() + TIMEOUT;
        while std::time::Instant::now() < deadline {
            match self.0.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

impl RemoteFixture {
    pub async fn start() -> Result<Self> {
        let binary = editor_binary("codex")?;
        // Local tests select this exact helper; the CLI's ordinary arg0 dispatch
        // selects its own source-identical built-in Linux sandbox alias remotely.
        editor_binary("codex-linux-sandbox")?;
        let home = TempDir::new()?;
        let process_cwd = TempDir::new()?;
        let child = Command::new(binary)
            .args(["exec-server", "--listen", "ws://127.0.0.1:0"])
            .env("CODEX_HOME", home.path())
            .current_dir(process_cwd.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("start the source-bound real exec-server")?;
        let mut child = ReapedChild(child);
        let stdout = child.0.stdout.take().context("exec-server stdout")?;
        let mut lines = BufReader::new(stdout).lines();
        let endpoint = tokio::time::timeout(TIMEOUT, async {
            for _ in 0..128 {
                let line = lines
                    .next_line()
                    .await?
                    .context("exec-server exited before readiness")?;
                if line.starts_with("ws://") {
                    return Ok::<_, anyhow::Error>(line);
                }
            }
            anyhow::bail!("exec-server did not emit a bounded listen endpoint")
        })
        .await
        .context("exec-server readiness timeout")??;
        let url = url::Url::parse(&endpoint)?;
        ensure!(
            url.scheme() == "ws"
                && url.host_str() == Some("127.0.0.1")
                && url.port().is_some_and(|port| port > 0),
            "unexpected child listen endpoint"
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let websocket_url = format!("ws://{}", listener.local_addr()?);
        let observation = Arc::new(Mutex::new(Observation::default()));
        let proxy_observation = Arc::clone(&observation);
        let proxy = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted?;
                        let endpoint = endpoint.clone();
                        let observation = Arc::clone(&proxy_observation);
                        connections.spawn(async move {
                            let mut client = accept_async(stream).await?;
                            let (mut server, _) = connect_async(&endpoint).await?;
                            let mut pending = HashMap::<String, usize>::new();
                            loop {
                                tokio::select! {
                                    message = client.next() => {
                                        let Some(message) = message else { break };
                                        let message = message?;
                                        if let Message::Text(text) = &message {
                                            let value: Value = serde_json::from_str(text)?;
                                            let method = value["method"].as_str().unwrap_or_default();
                                            let mut observed = observation.lock().expect("RPC observation lock");
                                            if method == "initialized" {
                                                observed.initialized = true;
                                            }
                                            let path = value.pointer("/params/path").and_then(Value::as_str);
                                            if matches!(method, "fs/readFile" | "fs/writeFileIfUnchanged" | "fs/writeFile")
                                                && path.is_some_and(|path| observed.paths.iter().any(|known| known == path)) {
                                                ensure!(observed.records.len() < 128, "RPC observation limit exceeded");
                                                let index = observed.records.len();
                                                observed.records.push(ObservedRpc {
                                                    method: method.to_owned(),
                                                    path: value.pointer("/params/path").and_then(Value::as_str)
                                                        .context("filesystem RPC has a path")?.to_owned(),
                                                    sandboxed: value.pointer("/params/sandbox").is_some_and(|sandbox| !sandbox.is_null()),
                                                    terminal: None,
                                                });
                                                pending.insert(value["id"].to_string(), index);
                                            }
                                        }
                                        let closed = message.is_close();
                                        server.send(message).await?;
                                        if closed { break; }
                                    }
                                    message = server.next() => {
                                        let Some(message) = message else { break };
                                        let message = message?;
                                        let mut drop_reply = false;
                                        if let Message::Text(text) = &message {
                                            let value: Value = serde_json::from_str(text)?;
                                            if let Some(index) = pending.remove(&value["id"].to_string()) {
                                                let mut observed = observation.lock().expect("RPC observation lock");
                                                let terminal = if let Some(error) = value.get("error") {
                                                    let message = error["message"].as_str().unwrap_or_default();
                                                    // Retain only a bounded classification, never the
                                                    // raw error/traffic or file contents. Startup
                                                    // failures cannot count as an intended denial.
                                                    if (message.contains("Permission denied (os error 13)")
                                                        || message.contains("Read-only file system (os error 30)"))
                                                        && !message.contains("fs sandbox helper failed")
                                                        && !message.contains("namespace")
                                                        && !message.contains("bwrap:")
                                                        && !message.contains("failed to prepare fs sandbox") {
                                                        RpcTerminal::FileAccessDenied
                                                    } else {
                                                        RpcTerminal::Error
                                                    }
                                                } else {
                                                    ensure!(value.get("result").is_some(), "RPC terminal response has no result");
                                                    if observed.records[index].method == "fs/writeFileIfUnchanged" {
                                                        match value.pointer("/result/written").and_then(Value::as_bool) {
                                                            Some(true) => RpcTerminal::Written,
                                                            Some(false) => RpcTerminal::Conflict,
                                                            None => anyhow::bail!("CAS response has no written boolean"),
                                                        }
                                                    } else { RpcTerminal::Success }
                                                };
                                                drop_reply = terminal == RpcTerminal::Written
                                                    && observed.drop_written_reply.as_deref() == Some(observed.records[index].path.as_str());
                                                observed.records[index].terminal = Some(terminal);
                                                if drop_reply {
                                                    observed.drop_written_reply = None;
                                                    observed.written_reply_dropped = true;
                                                }
                                            }
                                        }
                                        if drop_reply {
                                            // Observe an actual server result, then model a lost
                                            // terminal reply. Never synthesize a replacement.
                                            let _ = client.close(None).await;
                                            let _ = server.close(None).await;
                                            break;
                                        }
                                        let closed = message.is_close();
                                        client.send(message).await?;
                                        if closed { break; }
                                    }
                                }
                            }
                            Ok::<_, anyhow::Error>(())
                        });
                    }
                    finished = connections.join_next(), if !connections.is_empty() => {
                        finished.context("proxy connection task")??.context("transparent RPC forwarding")?;
                    }
                }
            }
        });
        Ok(Self {
            child,
            proxy,
            observation,
            websocket_url,
            _home: home,
            _process_cwd: process_cwd,
        })
    }

    pub fn url(&self) -> &str {
        &self.websocket_url
    }

    pub fn observe_path(&self, path: &codex_utils_path_uri::PathUri) {
        self.observation
            .lock()
            .expect("RPC observation lock")
            .paths
            .push(path.to_string());
    }

    pub fn drop_written_reply_once(&self, path: &codex_utils_path_uri::PathUri) {
        self.observation
            .lock()
            .expect("RPC observation lock")
            .drop_written_reply = Some(path.to_string());
    }

    pub fn assert_written_reply_dropped(&self) {
        assert!(
            self.observation
                .lock()
                .expect("RPC observation lock")
                .written_reply_dropped,
            "the actual child's successful CAS response must have reached the fault boundary"
        );
    }

    pub fn records(&self) -> Vec<ObservedRpc> {
        self.observation
            .lock()
            .expect("RPC observation lock")
            .records
            .clone()
    }

    pub fn assert_ready(&self) {
        assert!(
            self.observation
                .lock()
                .expect("RPC observation lock")
                .initialized,
            "real public initialization must have reached the child"
        );
        assert!(!self.proxy.is_finished(), "transparent RPC proxy failed");
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.proxy.is_finished() {
            (&mut self.proxy)
                .await
                .context("transparent RPC proxy task")??;
        } else {
            self.proxy.abort();
            let result = (&mut self.proxy).await;
            ensure!(
                result.is_err_and(|error| error.is_cancelled()),
                "proxy shutdown was not cancellation"
            );
        }
        self.child.0.start_kill()?;
        tokio::time::timeout(TIMEOUT, self.child.0.wait())
            .await
            .context("exec-server cleanup timeout")??;
        Ok(())
    }
}

impl Drop for RemoteFixture {
    fn drop(&mut self) {
        self.proxy.abort();
    }
}
