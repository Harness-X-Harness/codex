use std::io;

use anyhow::Context;
use anyhow::Result;
use codex_exec_server::ConditionalWriteResult;
use codex_exec_server::WriteFileOptions;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use test_case::test_case;

use crate::support::FileSystemImplementation;
use crate::support::create_file_system_context;
use crate::support::read_only_sandbox;
use crate::support::workspace_write_sandbox;

#[test_case(FileSystemImplementation::Local; "local")]
#[test_case(FileSystemImplementation::Remote; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_write_preserves_exact_bytes_and_rejects_stale_snapshots(
    implementation: FileSystemImplementation,
) -> Result<()> {
    let context = create_file_system_context(implementation).await?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let path = root.join("bytes");
    let uri = PathUri::from_host_native_path(&path)?;
    let before = b"first\r\n\0\xfflast";
    let after = b"second\r\n\0\xfelast";
    std::fs::write(&path, before)?;
    let fs = &context.file_system;
    assert_eq!(
        fs.write_file_if_unchanged(
            &uri,
            before.to_vec(),
            after.to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None
        )
        .await?,
        ConditionalWriteResult::Written,
    );
    assert_eq!(
        fs.write_file_if_unchanged(
            &uri,
            before.to_vec(),
            b"stale".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None
        )
        .await?,
        ConditionalWriteResult::Conflict,
    );
    assert_eq!(std::fs::read(&path)?, after);
    let missing = PathUri::from_host_native_path(root.join("missing"))?;
    assert_eq!(
        fs.write_file_if_unchanged(
            &missing,
            Vec::new(),
            b"must not create".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None
        )
        .await
        .expect_err("missing snapshot must not become an ordinary write")
        .kind(),
        io::ErrorKind::NotFound,
    );
    assert!(!root.join("missing").exists());
    Ok(())
}

#[test_case(FileSystemImplementation::Local; "local")]
#[test_case(FileSystemImplementation::Remote; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn competing_conditional_writes_have_one_snapshot_winner(
    implementation: FileSystemImplementation,
) -> Result<()> {
    let context = create_file_system_context(implementation).await?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let path = root.join("bytes");
    let uri = PathUri::from_host_native_path(&path)?;
    std::fs::write(&path, b"before")?;
    let fs = &context.file_system;
    let (first, second) = tokio::join!(
        fs.write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"first".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None
        ),
        fs.write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"second".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None
        ),
    );
    let first = first?;
    let second = second?;
    let bytes = std::fs::read(path)?;
    assert!(
        (first, second, bytes.as_slice())
            == (
                ConditionalWriteResult::Written,
                ConditionalWriteResult::Conflict,
                b"first".as_slice()
            )
            || (first, second, bytes.as_slice())
                == (
                    ConditionalWriteResult::Conflict,
                    ConditionalWriteResult::Written,
                    b"second".as_slice()
                ),
        "one expected snapshot must win: {first:?}, {second:?}, {bytes:?}",
    );
    Ok(())
}

#[test_case(FileSystemImplementation::Local; "local")]
#[test_case(FileSystemImplementation::Remote; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_write_uses_real_sandbox_and_releases_after_settled_denial(
    implementation: FileSystemImplementation,
) -> Result<()> {
    let context = create_file_system_context(implementation).await?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let path = root.join("bytes");
    let uri = PathUri::from_host_native_path(&path)?;
    std::fs::write(&path, b"before")?;
    let sandbox = read_only_sandbox(root.clone());
    let fs = &context.file_system;
    let error = fs
        .write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"denied".to_vec(),
            WriteFileOptions {
                follow_symlinks: false,
            },
            Some(&sandbox),
        )
        .await
        .expect_err("configured sandbox must deny the actual write");
    let message = error.to_string().to_lowercase();
    assert!(
        message.contains("permission denied")
            || message.contains("access is denied")
            || message.contains("read-only file system")
            || message.contains("operation not permitted")
            || message.contains("is not permitted"),
        "required native sandbox enforcement unavailable or unexpected denial ({implementation}): {error}",
    );
    assert_eq!(std::fs::read(&path)?, b"before");
    let sandbox = workspace_write_sandbox(root);
    assert_eq!(
        fs.write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"allowed".to_vec(),
            WriteFileOptions {
                follow_symlinks: false
            },
            Some(&sandbox)
        )
        .await
        .with_context(|| format!("required native sandbox write ({implementation})"))?,
        ConditionalWriteResult::Written,
    );
    assert_eq!(std::fs::read(path)?, b"allowed");
    Ok(())
}

#[test_case(FileSystemImplementation::Local; "local")]
#[test_case(FileSystemImplementation::Remote; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_write_preserves_hard_links_and_rejects_no_follow_links(
    implementation: FileSystemImplementation,
) -> Result<()> {
    let context = create_file_system_context(implementation).await?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let actual = root.join("actual");
    std::fs::create_dir(&actual)?;
    let path = actual.join("bytes");
    let alias = root.join("hard-link");
    let link = root.join("symbolic-link");
    std::fs::write(&path, b"before")?;
    std::fs::hard_link(&path, &alias)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &link)?;
    #[cfg(windows)]
    let link = {
        // Use the stock junction fixture without requesting symlink privileges or weakening policy.
        crate::create_directory_junction(&actual, &link)
            .context("required Windows junction fixture is unavailable")?;
        link.join("bytes")
    };
    let uri = PathUri::from_host_native_path(&path)?;
    let fs = &context.file_system;
    assert_eq!(
        fs.write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"after".to_vec(),
            WriteFileOptions {
                follow_symlinks: false
            },
            /*sandbox*/ None
        )
        .await?,
        ConditionalWriteResult::Written,
    );
    assert_eq!(std::fs::read(&alias)?, b"after");
    fs.write_file_if_unchanged(
        &PathUri::from_host_native_path(&link)?,
        b"after".to_vec(),
        b"must not follow".to_vec(),
        WriteFileOptions {
            follow_symlinks: false,
        },
        /*sandbox*/ None,
    )
    .await
    .expect_err("no-follow comparison must reject a symbolic link");
    assert_eq!(std::fs::read(&path)?, b"after");
    assert_eq!(
        fs.write_file_if_unchanged(
            &PathUri::from_host_native_path(&link)?,
            b"after".to_vec(),
            b"followed".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None,
        )
        .await?,
        ConditionalWriteResult::Written,
    );
    assert_eq!(std::fs::read(&path)?, b"followed");
    assert_eq!(std::fs::read(&alias)?, b"followed");
    Ok(())
}

#[cfg(unix)]
#[test_case(FileSystemImplementation::Local; "local")]
#[test_case(FileSystemImplementation::Remote; "remote")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_write_denies_unreadable_snapshot_before_comparison(
    implementation: FileSystemImplementation,
) -> Result<()> {
    let context = create_file_system_context(implementation).await?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let allowed = root.join("allowed");
    let outside = root.join("outside");
    std::fs::create_dir(&allowed)?;
    std::fs::create_dir(&outside)?;
    let path = outside.join("secret");
    std::fs::write(&path, b"secret snapshot")?;
    std::os::unix::fs::symlink(&outside, allowed.join("link"))?;
    let escaped = allowed.join("link").join("secret");
    let sandbox = read_only_sandbox(allowed);
    for requested in [&path, &escaped] {
        let error = context
            .file_system
            .write_file_if_unchanged(
                &PathUri::from_host_native_path(requested)?,
                b"deliberately different snapshot".to_vec(),
                b"must not write".to_vec(),
                WriteFileOptions::default(),
                Some(&sandbox),
            )
            .await
            .expect_err("sandbox must reject an unreadable snapshot instead of reporting conflict");
        crate::assert_sandbox_denied(&error);
        assert_eq!(std::fs::read(&path)?, b"secret snapshot");
    }
    Ok(())
}
