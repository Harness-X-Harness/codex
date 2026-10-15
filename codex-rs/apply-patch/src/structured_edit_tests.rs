use std::io;
use std::sync::Mutex;

use codex_exec_server::CopyOptions;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::ExecutorFileSystemFuture;
use codex_exec_server::FileMetadata;
use codex_exec_server::FileSystemReadStream;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::LOCAL_FS;
use codex_exec_server::ReadDirectoryEntry;
use codex_exec_server::RemoveOptions;
use codex_exec_server::WalkOptions;
use codex_exec_server::WalkOutcome;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

use super::StructuredEditError::EmptyOldString;
use super::StructuredEditError::MultipleMatches;
use super::StructuredEditError::OldEqualsNew;
use super::StructuredEditError::ZeroMatches;
use super::*;

#[test]
fn structured_edit_exact_matching_preserves_bytes_and_nonoverlapping_semantics() {
    for (content, old, new, replace_all, expected) in [
        ("hello world", "world", "there", false, "hello there"),
        ("aa aa aa", "aa", "bb", true, "bb bb bb"),
        ("aaa", "aa", "b", false, "ba"),
        ("aaaaa", "aa", "b", true, "bba"),
        ("café\n雪", "café", "🙂", false, "🙂\n雪"),
        ("hello\r\nworld", "hello", "hi", false, "hi\r\nworld"),
        ("\u{feff}x\r\ny\n", "x", "", false, "\u{feff}\r\ny\n"),
        ("a  b\t c", "b", "B", false, "a  B\t c"),
        ("a\r\nb\n", "\r\n", "\n", false, "a\nb\n"),
        ("x", "x", "", false, ""),
    ] {
        assert_eq!(
            apply_exact_replacement(content, old, new, replace_all),
            Ok(expected.to_owned()),
            "input={content:?}"
        );
    }
}

#[test]
fn structured_edit_cardinality_and_exact_matching_errors() {
    for (content, old, new, replace_all, error) in [
        ("hello", "", "x", false, EmptyOldString),
        ("hello", "hello", "hello", true, OldEqualsNew),
        ("hello", "missing", "x", false, ZeroMatches),
        ("hello", "missing", "x", true, ZeroMatches),
        ("hello\r\n", "hello\n", "x", false, ZeroMatches),
        ("café", "cafe\u{301}", "x", false, ZeroMatches),
        ("a  b", "a b", "x", false, ZeroMatches),
        ("aa aa", "aa", "bb", false, MultipleMatches { count: 2 }),
    ] {
        assert_eq!(
            apply_exact_replacement(content, old, new, replace_all),
            Err(error),
            "input={content:?}"
        );
    }
}

#[tokio::test]
async fn structured_edit_verified_write_commits_exact_bytes_and_delta() {
    for (old, new) in [
        ("before\r\nlast", "after\r\nlast"),
        ("before\n", "after"),
        ("before", "after\n"),
        ("\u{feff}café\r\n雪\n", "\u{feff}cafe\r\n雪\n"),
        ("before", ""),
    ] {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = dir.path().canonicalize().expect("canonical temporary root");
        let path = root.join("file.txt");
        std::fs::write(&path, old).expect("seed");
        let cwd = PathUri::from_host_native_path(&root).expect("cwd");
        let uri = PathUri::from_host_native_path(&path).expect("path URI");
        let mut action = ApplyPatchAction::from_exact_update(cwd, uri.clone(), old, new.to_owned());
        // The patch is presentation only, even if it cannot be parsed at all.
        action.patch = "not a patch".to_owned();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let delta = apply_verified_action(
            &action,
            ApplyPatchOptions::default(),
            &mut stdout,
            &mut stderr,
            LOCAL_FS.as_ref(),
            /*sandbox*/ None,
        )
        .await
        .expect("exact write");
        assert_eq!(std::fs::read(&path).expect("final bytes"), new.as_bytes());
        assert_eq!(delta, committed_delta(&uri, old, new));
        assert_eq!(
            stdout,
            b"Success. Updated the following files:\nM file.txt\n"
        );
        assert!(stderr.is_empty());
    }
}

#[tokio::test]
async fn structured_edit_verified_write_rejects_change_after_final_read() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let root = dir.path().canonicalize().expect("canonical temporary root");
    let path = root.join("file.txt");
    std::fs::write(&path, "before").expect("seed");
    let cwd = PathUri::from_host_native_path(&root).expect("cwd");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let action =
        ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
    let fs = ObservedFileSystem::new(WriteBehavior::ChangeAfterRead);
    let error = run(&action, &fs).await.expect_err("CAS conflict");
    assert!(error.to_string().contains(STALE_STRUCTURED_EDIT_MESSAGE));
    assert_eq!(error.delta(), &AppliedPatchDelta::default());
    assert_eq!(std::fs::read(&path).expect("final bytes"), b"concurrent");
    assert_eq!(fs.calls(), expected_calls(&uri, "before", "after"));
}

#[tokio::test]
async fn structured_edit_verified_write_rejects_stale_snapshot_before_cas() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let root = dir.path().canonicalize().expect("canonical temporary root");
    let path = root.join("file.txt");
    std::fs::write(&path, "concurrent").expect("changed after planning");
    let cwd = PathUri::from_host_native_path(&root).expect("cwd");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let action =
        ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
    let fs = ObservedFileSystem::new(WriteBehavior::Normal);
    let error = run(&action, &fs).await.expect_err("stale snapshot");
    assert!(error.to_string().contains(STALE_STRUCTURED_EDIT_MESSAGE));
    assert_eq!(error.delta(), &AppliedPatchDelta::default());
    assert_eq!(
        fs.calls(),
        vec![Call::Read(
            uri,
            ReadFileOptions {
                follow_symlinks: false
            }
        )]
    );
    assert_eq!(std::fs::read(&path).expect("final bytes"), b"concurrent");
}

#[tokio::test]
async fn structured_edit_verified_write_preserves_uncertain_effect_without_retry() {
    for (behavior, final_bytes, error_kind) in [
        (
            WriteBehavior::Denied,
            b"before".as_slice(),
            io::ErrorKind::PermissionDenied,
        ),
        (
            WriteBehavior::Unsupported,
            b"before".as_slice(),
            io::ErrorKind::Unsupported,
        ),
        (
            WriteBehavior::DisconnectAfterCommit,
            b"after".as_slice(),
            io::ErrorKind::ConnectionReset,
        ),
    ] {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = dir.path().canonicalize().expect("canonical temporary root");
        let path = root.join("file.txt");
        std::fs::write(&path, "before").expect("seed");
        let cwd = PathUri::from_host_native_path(&root).expect("cwd");
        let uri = PathUri::from_host_native_path(&path).expect("path URI");
        let action =
            ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
        let fs = ObservedFileSystem::new(behavior);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let error = apply_verified_action(
            &action,
            ApplyPatchOptions {
                follow_symlinks: false,
                ..ApplyPatchOptions::default()
            },
            &mut stdout,
            &mut stderr,
            &fs,
            /*sandbox*/ None,
        )
        .await
        .expect_err("write failure");
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).expect("diagnostic UTF-8"),
            format!(
                "Failed to write file {}: {}\n",
                uri.inferred_native_path_string(),
                io::Error::from(error_kind)
            ),
        );
        assert_eq!(
            error.delta(),
            &AppliedPatchDelta::new(Vec::new(), /*exact*/ false)
        );
        assert!(!error.is_retryable_read_denial());
        assert_eq!(fs.calls(), expected_calls(&uri, "before", "after"));
        assert_eq!(std::fs::read(&path).expect("final bytes"), final_bytes);
    }
}

#[tokio::test]
async fn structured_edit_retry_requires_native_pre_cas_read_denial() {
    for (kind, retryable) in [
        (io::ErrorKind::PermissionDenied, true),
        (io::ErrorKind::BrokenPipe, false),
        (io::ErrorKind::Other, false),
        (io::ErrorKind::InvalidInput, false),
    ] {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = dir.path().canonicalize().expect("canonical temporary root");
        let path = root.join("file.txt");
        std::fs::write(&path, "before").expect("seed");
        let cwd = PathUri::from_host_native_path(&root).expect("cwd");
        let uri = PathUri::from_host_native_path(&path).expect("path URI");
        let action =
            ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
        let fs = ObservedFileSystem::new(WriteBehavior::ReadError(kind));
        let error = run(&action, &fs).await.expect_err("read failure");
        assert_eq!(error.is_retryable_read_denial(), retryable, "kind={kind:?}");
        assert_eq!(
            error.delta(),
            &AppliedPatchDelta::new(Vec::new(), /*exact*/ false)
        );
        assert_eq!(
            fs.calls(),
            vec![Call::Read(
                uri,
                ReadFileOptions {
                    follow_symlinks: false
                }
            )]
        );
        assert_eq!(std::fs::read(&path).expect("unchanged bytes"), b"before");
    }
}

#[tokio::test]
async fn structured_edit_verified_write_read_failures_never_create_or_overwrite() {
    for bytes in [None, Some(vec![0xff])] {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = dir.path().canonicalize().expect("canonical temporary root");
        let path = root.join("file.txt");
        if let Some(bytes) = &bytes {
            std::fs::write(&path, bytes).expect("invalid UTF-8 seed");
        }
        let cwd = PathUri::from_host_native_path(&root).expect("cwd");
        let uri = PathUri::from_host_native_path(&path).expect("path URI");
        let action =
            ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
        let fs = ObservedFileSystem::new(WriteBehavior::Normal);
        let error = run(&action, &fs).await.expect_err("read failure");
        assert_eq!(
            error.delta(),
            &AppliedPatchDelta::new(Vec::new(), /*exact*/ false)
        );
        assert_eq!(
            fs.calls(),
            vec![Call::Read(
                uri,
                ReadFileOptions {
                    follow_symlinks: false
                }
            )]
        );
        assert_eq!(std::fs::read(&path).ok(), bytes);
    }
}

#[tokio::test]
async fn structured_edit_verified_write_rejects_unsupported_actions_before_io() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let root = dir.path().canonicalize().expect("canonical temporary root");
    let path = root.join("file.txt");
    std::fs::write(&path, "before").expect("seed");
    let cwd = PathUri::from_host_native_path(&root).expect("cwd");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let dest = cwd.join("dest.txt").expect("move path");
    for change in [
        ApplyPatchFileChange::Add {
            content: "after".to_owned(),
        },
        ApplyPatchFileChange::Delete {
            content: "before".to_owned(),
        },
        ApplyPatchFileChange::Update {
            unified_diff: String::new(),
            move_path: None,
            new_content: "after".to_owned(),
            expected_content: None,
        },
        ApplyPatchFileChange::Update {
            unified_diff: String::new(),
            move_path: Some(dest.clone()),
            new_content: "after".to_owned(),
            expected_content: Some("before".to_owned()),
        },
    ] {
        let mut action = ApplyPatchAction::from_exact_update(
            cwd.clone(),
            uri.clone(),
            "before",
            "after".to_owned(),
        );
        action.changes.insert(uri.clone(), change);
        let fs = ObservedFileSystem::new(WriteBehavior::Normal);
        let error = run(&action, &fs).await.expect_err("unsupported action");
        assert_eq!(error.delta(), &AppliedPatchDelta::default());
        assert_eq!(fs.calls(), Vec::new());
    }
    let mut action = ApplyPatchAction::from_exact_update(cwd, uri, "before", "after".to_owned());
    action.changes.insert(
        dest,
        ApplyPatchFileChange::Add {
            content: "another".to_owned(),
        },
    );
    let fs = ObservedFileSystem::new(WriteBehavior::Normal);
    assert_eq!(
        run(&action, &fs).await.expect_err("multiple files").delta(),
        &AppliedPatchDelta::default()
    );
    action.changes.clear();
    assert_eq!(
        run(&action, &fs).await.expect_err("empty action").delta(),
        &AppliedPatchDelta::default()
    );
    assert_eq!(fs.calls(), Vec::new());
    assert_eq!(std::fs::read(&path).expect("final bytes"), b"before");
    assert!(!dir.path().join("dest.txt").exists());
}

#[tokio::test]
async fn structured_edit_output_failure_retains_committed_delta() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let root = dir.path().canonicalize().expect("canonical temporary root");
    let path = root.join("file.txt");
    std::fs::write(&path, "before").expect("seed");
    let cwd = PathUri::from_host_native_path(&root).expect("cwd");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let action =
        ApplyPatchAction::from_exact_update(cwd, uri.clone(), "before", "after".to_owned());
    let error = apply_verified_action(
        &action,
        ApplyPatchOptions::default(),
        &mut RejectOutput,
        &mut Vec::new(),
        LOCAL_FS.as_ref(),
        /*sandbox*/ None,
    )
    .await
    .expect_err("summary output failure");
    assert_eq!(error.delta(), &committed_delta(&uri, "before", "after"));
    assert_eq!(std::fs::read(&path).expect("final bytes"), b"after");
}

fn committed_delta(path: &PathUri, old: &str, new: &str) -> AppliedPatchDelta {
    AppliedPatchDelta::new(
        vec![AppliedPatchChange {
            path: path.clone(),
            change: AppliedPatchFileChange::Update {
                move_path: None,
                old_content: old.to_owned(),
                overwritten_move_content: None,
                new_content: new.to_owned(),
            },
        }],
        /*exact*/ true,
    )
}

async fn run(
    action: &ApplyPatchAction,
    fs: &ObservedFileSystem,
) -> Result<AppliedPatchDelta, ApplyPatchFailure> {
    apply_verified_action(
        action,
        ApplyPatchOptions {
            follow_symlinks: false,
            ..ApplyPatchOptions::default()
        },
        &mut Vec::new(),
        &mut Vec::new(),
        fs,
        /*sandbox*/ None,
    )
    .await
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Read(PathUri, ReadFileOptions),
    Conditional(PathUri, Vec<u8>, Vec<u8>, WriteFileOptions),
}

fn expected_calls(path: &PathUri, old: &str, new: &str) -> Vec<Call> {
    vec![
        Call::Read(
            path.clone(),
            ReadFileOptions {
                follow_symlinks: false,
            },
        ),
        Call::Conditional(
            path.clone(),
            old.as_bytes().to_vec(),
            new.as_bytes().to_vec(),
            WriteFileOptions {
                follow_symlinks: false,
            },
        ),
    ]
}

enum WriteBehavior {
    Normal,
    ReadError(io::ErrorKind),
    ChangeAfterRead,
    Denied,
    Unsupported,
    DisconnectAfterCommit,
}

struct ObservedFileSystem {
    behavior: WriteBehavior,
    calls: Mutex<Vec<Call>>,
}

impl ObservedFileSystem {
    fn new(behavior: WriteBehavior) -> Self {
        Self {
            behavior,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl ExecutorFileSystem for ObservedFileSystem {
    fn read_file<'a>(
        &'a self,
        path: &'a PathUri,
        options: ReadFileOptions,
        sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<u8>> {
        Box::pin(async move {
            assert!(sandbox.is_none());
            self.calls
                .lock()
                .expect("calls lock")
                .push(Call::Read(path.clone(), options));
            if let WriteBehavior::ReadError(kind) = self.behavior {
                return Err(io::Error::new(
                    kind,
                    "sandbox permission denied: untrusted diagnostic",
                ));
            }
            let bytes = LOCAL_FS.read_file(path, options, sandbox).await?;
            if matches!(self.behavior, WriteBehavior::ChangeAfterRead) {
                // The caller receives the old snapshot although the target has changed.
                std::fs::write(path.to_abs_path()?, b"concurrent")?;
            }
            Ok(bytes)
        })
    }

    fn write_file_if_unchanged<'a>(
        &'a self,
        path: &'a PathUri,
        expected_contents: Vec<u8>,
        contents: Vec<u8>,
        options: WriteFileOptions,
        sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ConditionalWriteResult> {
        Box::pin(async move {
            assert!(sandbox.is_none());
            self.calls
                .lock()
                .expect("calls lock")
                .push(Call::Conditional(
                    path.clone(),
                    expected_contents.clone(),
                    contents.clone(),
                    options,
                ));
            match self.behavior {
                WriteBehavior::ReadError(_) => panic!("read failure must precede every CAS"),
                WriteBehavior::Denied => {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                WriteBehavior::Unsupported => {
                    return Err(io::Error::from(io::ErrorKind::Unsupported));
                }
                WriteBehavior::Normal
                | WriteBehavior::ChangeAfterRead
                | WriteBehavior::DisconnectAfterCommit => {}
            }
            let result = LOCAL_FS
                .write_file_if_unchanged(path, expected_contents, contents, options, sandbox)
                .await?;
            if matches!(self.behavior, WriteBehavior::DisconnectAfterCommit) {
                assert_eq!(result, ConditionalWriteResult::Written);
                return Err(io::Error::from(io::ErrorKind::ConnectionReset));
            }
            Ok(result)
        })
    }

    fn write_file<'a>(
        &'a self,
        _: &'a PathUri,
        _: Vec<u8>,
        _: WriteFileOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        panic!("exact updates must never fall back to an ordinary write")
    }

    fn canonicalize<'a>(
        &'a self,
        _: &'a PathUri,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, PathUri> {
        panic!("unexpected filesystem call")
    }

    fn read_file_stream<'a>(
        &'a self,
        _: &'a PathUri,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileSystemReadStream> {
        panic!("unexpected filesystem call")
    }

    fn create_directory<'a>(
        &'a self,
        _: &'a PathUri,
        _: CreateDirectoryOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        panic!("unexpected filesystem call")
    }

    fn get_metadata<'a>(
        &'a self,
        _: &'a PathUri,
        _: GetMetadataOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileMetadata> {
        panic!("unexpected filesystem call")
    }

    fn read_directory<'a>(
        &'a self,
        _: &'a PathUri,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<ReadDirectoryEntry>> {
        panic!("unexpected filesystem call")
    }

    fn walk<'a>(
        &'a self,
        _: &'a PathUri,
        _: WalkOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, WalkOutcome> {
        panic!("unexpected filesystem call")
    }

    fn remove<'a>(
        &'a self,
        _: &'a PathUri,
        _: RemoveOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        panic!("unexpected filesystem call")
    }

    fn copy<'a>(
        &'a self,
        _: &'a PathUri,
        _: &'a PathUri,
        _: CopyOptions,
        _: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        panic!("unexpected filesystem call")
    }
}

struct RejectOutput;

impl io::Write for RejectOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
