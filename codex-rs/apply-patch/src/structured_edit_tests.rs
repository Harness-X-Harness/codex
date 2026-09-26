use pretty_assertions::assert_eq;

use super::StructuredEditError;
use super::apply_exact_replacement;

#[test]
fn structured_edit_replaces_a_single_exact_match() {
    assert_eq!(
        apply_exact_replacement("hello world", "world", "there", /*replace_all*/ false)
            .expect("single match"),
        "hello there"
    );
}

#[test]
fn structured_edit_rejects_zero_matches() {
    assert_eq!(
        apply_exact_replacement(
            "hello world",
            "missing",
            "there",
            /*replace_all*/ false
        ),
        Err(StructuredEditError::ZeroMatches)
    );
}

#[test]
fn structured_edit_rejects_multiple_matches_without_replace_all() {
    assert_eq!(
        apply_exact_replacement("aa aa", "aa", "bb", /*replace_all*/ false),
        Err(StructuredEditError::MultipleMatches { count: 2 })
    );
}

#[test]
fn structured_edit_replace_all_replaces_every_exact_match() {
    assert_eq!(
        apply_exact_replacement("aa aa aa", "aa", "bb", /*replace_all*/ true).expect("replace_all"),
        "bb bb bb"
    );
}

#[test]
fn structured_edit_rejects_empty_old_string() {
    assert_eq!(
        apply_exact_replacement("hello", "", "x", /*replace_all*/ false),
        Err(StructuredEditError::EmptyOldString)
    );
}

#[test]
fn structured_edit_rejects_identical_old_and_new() {
    assert_eq!(
        apply_exact_replacement("hello", "hello", "hello", /*replace_all*/ true),
        Err(StructuredEditError::OldEqualsNew)
    );
}

#[test]
fn structured_edit_matches_unicode_exactly() {
    assert_eq!(
        apply_exact_replacement("café café", "café", "cafe", /*replace_all*/ true)
            .expect("unicode"),
        "cafe cafe"
    );
}

#[test]
fn structured_edit_treats_crlf_as_exact_bytes() {
    assert_eq!(
        apply_exact_replacement(
            "hello\r\nworld",
            "hello\n",
            "hi\n",
            /*replace_all*/ false
        ),
        Err(StructuredEditError::ZeroMatches)
    );
    assert_eq!(
        apply_exact_replacement(
            "hello\r\nworld",
            "hello\r\n",
            "hi\r\n",
            /*replace_all*/ false
        )
        .expect("crlf match"),
        "hi\r\nworld"
    );
}

#[test]
fn structured_edit_does_not_count_overlapping_matches() {
    assert_eq!(
        apply_exact_replacement("aaa", "aa", "b", /*replace_all*/ false).expect("non-overlapping"),
        "ba"
    );
}

#[tokio::test]
async fn structured_edit_verified_write_preserves_crlf_bytes() {
    use crate::ApplyPatchAction;
    use crate::ApplyPatchOptions;
    use crate::apply_verified_action;
    use codex_exec_server::LOCAL_FS;
    use codex_utils_path_uri::PathUri;
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("crlf.txt");
    fs::write(&path, "hello\r\nworld").expect("seed");
    let path_uri = PathUri::from_host_native_path(&path).expect("path uri");
    let cwd = PathUri::from_host_native_path(dir.path()).expect("cwd");
    let new_content =
        apply_exact_replacement("hello\r\nworld", "hello", "hi", /*replace_all*/ false)
            .expect("match");
    let action = ApplyPatchAction::from_exact_update(cwd, path_uri, "hello\r\nworld", new_content);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_verified_action(
        &action,
        ApplyPatchOptions {
            follow_symlinks: true,
            ..ApplyPatchOptions::default()
        },
        &mut stdout,
        &mut stderr,
        LOCAL_FS.as_ref(),
        None,
    )
    .await
    .expect("verified write");

    assert_eq!(fs::read(&path).expect("read"), b"hi\r\nworld");
}
