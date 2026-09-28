use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use tokio::io;

use super::*;

#[tokio::test]
async fn direct_file_system_rejects_non_native_uri_as_invalid_input() {
    let error = DirectFileSystem
        .read_file(&non_native_uri(), Default::default(), /*sandbox*/ None)
        .await
        .expect_err("non-native URI should be rejected");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

fn non_native_uri() -> PathUri {
    #[cfg(unix)]
    let uri = "file://server/share/file.txt";
    #[cfg(windows)]
    let uri = "file:///usr/local/file.txt";

    match PathUri::parse(uri) {
        Ok(uri) => uri,
        Err(err) => panic!("valid non-native URI should parse: {err}"),
    }
}

#[tokio::test]
async fn conditional_write_serializes_competing_writers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = PathUri::from_host_native_path(temp.path().join("cas.txt")).expect("path URI");
    let file_system = LocalFileSystem::unsandboxed();
    file_system
        .write_file(
            &path,
            b"version-a\n".to_vec(),
            Default::default(),
            /*sandbox*/ None,
        )
        .await
        .expect("seed file");

    let first = file_system.write_file_if_unchanged(
        &path,
        b"version-a\n".to_vec(),
        b"version-b\n".to_vec(),
        Default::default(),
        /*sandbox*/ None,
    );
    let second = file_system.write_file_if_unchanged(
        &path,
        b"version-a\n".to_vec(),
        b"version-c\n".to_vec(),
        Default::default(),
        /*sandbox*/ None,
    );
    let (first, second) = tokio::join!(first, second);
    let first = first.expect("first conditional write");
    let second = second.expect("second conditional write");

    assert!(
        matches!(
            (first, second),
            (
                ConditionalWriteResult::Written,
                ConditionalWriteResult::Conflict
            ) | (
                ConditionalWriteResult::Conflict,
                ConditionalWriteResult::Written
            )
        ),
        "exactly one competing writer must commit: first={first:?} second={second:?}"
    );

    let contents = file_system
        .read_file(&path, Default::default(), /*sandbox*/ None)
        .await
        .expect("read committed file");
    match (first, second) {
        (ConditionalWriteResult::Written, ConditionalWriteResult::Conflict) => {
            assert_eq!(contents, b"version-b\n");
        }
        (ConditionalWriteResult::Conflict, ConditionalWriteResult::Written) => {
            assert_eq!(contents, b"version-c\n");
        }
        _ => unreachable!("assertion above covers the only valid outcomes"),
    }
}
