//! Atomic, bounded Workflow persist reads and storage-failure behavior.

use std::fs::File;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::MAX_WORKFLOW_PERSIST_BYTES;
use codex_workflow_extension::PersistError;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowServiceError;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::load_workflow_document;
use codex_workflow_extension::persist_workflow_document;

fn yield_source() -> &'static str {
    r#"
        ask("continue");
        complete();
    "#
}

#[test]
fn torn_staging_leaves_the_previous_final_document() {
    let dir = TempDir::new().expect("temp");
    persist_workflow_document(dir.path(), "thread-a", br#"{"ok":true}"#).expect("first");
    std::fs::write(dir.path().join("thread-a.json.tmp"), b"{").expect("torn staging");
    let loaded = load_workflow_document(dir.path(), "thread-a").expect("load");
    assert_eq!(loaded, Some(br#"{"ok":true}"#.to_vec()));
}

#[test]
fn successful_commit_replaces_the_final_document() {
    let dir = TempDir::new().expect("temp");
    persist_workflow_document(dir.path(), "thread-a", br#"{"n":1}"#).expect("first");
    persist_workflow_document(dir.path(), "thread-a", br#"{"n":2}"#).expect("second");
    let loaded = load_workflow_document(dir.path(), "thread-a").expect("load");
    assert_eq!(loaded, Some(br#"{"n":2}"#.to_vec()));
}

#[test]
fn oversized_persist_file_is_rejected_from_metadata_size() {
    let dir = TempDir::new().expect("temp");
    let path = dir.path().join("huge.json");
    let file = File::create(&path).expect("create");
    file.set_len(MAX_WORKFLOW_PERSIST_BYTES as u64 + 1)
        .expect("sparse");
    drop(file);
    let error = load_workflow_document(dir.path(), "huge").expect_err("oversize");
    assert_eq!(
        error,
        PersistError::TooLarge {
            actual: MAX_WORKFLOW_PERSIST_BYTES as u64 + 1,
        }
    );
}

#[cfg(unix)]
#[test]
fn symlink_persist_file_is_rejected() {
    let dir = TempDir::new().expect("temp");
    let target = dir.path().join("outside.json");
    std::fs::write(&target, br#"{"secret":true}"#).expect("target");
    std::os::unix::fs::symlink(&target, dir.path().join("demo.json")).expect("symlink");
    let error = persist_workflow_document(dir.path(), "demo", br#"{"n":1}"#).expect_err("symlink");
    assert!(matches!(error, PersistError::UnsafePath(_)), "{error:?}");
    assert_eq!(
        std::fs::read_to_string(&target).expect("target intact"),
        r#"{"secret":true}"#
    );
}

#[tokio::test]
async fn corrupt_final_json_stays_on_disk() {
    let dir = TempDir::new().expect("temp");
    let thread_id = ThreadId::from_u128(41);
    let path = dir.path().join(format!("{thread_id}.json"));
    std::fs::write(&path, b"{").expect("torn");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = service.get_run(thread_id).await.expect("load");
    assert_eq!(loaded.expect("unreadable").status, WorkflowStatus::Failed);
    assert_eq!(std::fs::read(&path).expect("preserved"), b"{");
}

#[tokio::test]
async fn oversized_args_are_rejected_before_a_run_is_created() {
    let dir = TempDir::new().expect("temp");
    std::fs::write(
        dir.path().join("demo.rhai"),
        r#"
            let meta = #{
                name: "demo",
                description: "persist demo",
            };
            complete();
        "#,
    )
    .expect("catalog");
    let mut args = serde_json::Map::new();
    args.insert(
        "blob".to_string(),
        serde_json::Value::String("x".repeat(MAX_WORKFLOW_PERSIST_BYTES)),
    );
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let error = service
        .start_named_run(ThreadId::from_u128(42), "demo", args)
        .await
        .expect_err("args cap");
    assert!(
        matches!(error, WorkflowServiceError::InvalidRequest(reason) if reason.contains("workflow args exceed")),
        "{error:?}"
    );
    assert!(
        !dir.path()
            .join(format!("{}.json", ThreadId::from_u128(42)))
            .exists()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn mutation_persist_failure_does_not_stay_active() {
    let dir = TempDir::new().expect("temp");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(43);
    let started = service
        .start_run(thread_id, yield_source())
        .await
        .expect("start");
    assert_eq!(started.status, WorkflowStatus::Active);
    let path = dir.path().join(format!("{thread_id}.json"));
    let before = std::fs::read(&path).expect("durable");

    let mut permissions = std::fs::metadata(dir.path()).expect("meta").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(dir.path(), permissions).expect("readonly");

    let error = service.stop_run(thread_id).await.expect_err("persist fail");
    assert!(
        matches!(error, WorkflowServiceError::Internal(_)),
        "{error:?}"
    );
    let live = service
        .get_run(thread_id)
        .await
        .expect("cached")
        .expect("run");
    assert_eq!(live.status, WorkflowStatus::Failed);
    assert_eq!(std::fs::read(&path).expect("unchanged"), before);

    let mut permissions = std::fs::metadata(dir.path()).expect("meta").permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(dir.path(), permissions).expect("restore");
}
