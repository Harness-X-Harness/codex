use std::fs::File;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::read_workflow_path_source;

#[test]
fn reads_a_regular_workflow_file() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("review.rhai");
    std::fs::write(&path, r#"complete();"#).expect("write");
    assert_eq!(
        read_workflow_path_source(&path).expect("read"),
        "complete();"
    );
}

#[test]
fn rejects_an_oversized_workflow_file_from_metadata() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("huge.rhai");
    let file = File::create(&path).expect("create");
    file.set_len(1_000_000).expect("sparse");
    drop(file);
    let error = read_workflow_path_source(&path).expect_err("oversize");
    assert!(
        error.contains("workflow source is") && error.contains("bytes"),
        "unexpected error: {error}"
    );
}
