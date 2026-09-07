use std::fs::File;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_workflow_extension::CatalogError;
use codex_workflow_extension::CatalogRoots;
use codex_workflow_extension::MAX_CATALOG_SCOPE_RHAI_FILES;
use codex_workflow_extension::MAX_WORKFLOW_SOURCE_BYTES;
use codex_workflow_extension::resolve_named;

fn write_script(dir: &std::path::Path, name: &str, source: &str) {
    std::fs::create_dir_all(dir).expect("dir");
    std::fs::write(dir.join(format!("{name}.rhai")), source).expect("write");
}

fn demo_source(name: &str) -> String {
    format!(
        r#"
        let meta = #{{
            name: "{name}",
            description: "demo script",
        }};
        complete();
        "#
    )
}

#[test]
fn named_start_loads_user_library() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    write_script(&home.path().join("workflows"), "demo", &demo_source("demo"));
    let roots = CatalogRoots::new(home.path(), project.path());
    let script = resolve_named("demo", &roots).expect("resolve");
    assert_eq!(script.name, "demo");
    assert!(script.source.contains("complete();"));
}

#[test]
fn project_library_wins_over_user_library() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    write_script(
        &home.path().join("workflows"),
        "demo",
        &demo_source("demo").replace("complete();", r#"ask("user"); complete();"#),
    );
    write_script(
        &project.path().join(".codex").join("workflows"),
        "demo",
        &demo_source("demo"),
    );
    let roots = CatalogRoots::new(home.path(), project.path());
    let script = resolve_named("demo", &roots).expect("resolve");
    assert!(
        !script.source.contains(r#"ask("user")"#),
        "project script should win: {}",
        script.source
    );
}

#[test]
fn filename_must_match_meta_name() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    write_script(
        &home.path().join("workflows"),
        "demo",
        &demo_source("other"),
    );
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("demo", &roots).expect_err("mismatch");
    match error {
        CatalogError::FilenameMismatch { filename, name } => {
            assert_eq!(filename, "demo.rhai");
            assert_eq!(name, "other");
        }
        other => panic!("expected FilenameMismatch, got {other:?}"),
    }
}

#[test]
fn catalog_does_not_read_grok_home() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let grok = home.path().join(".grok").join("workflows");
    write_script(&grok, "demo", &demo_source("demo"));
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("demo", &roots).expect_err("grok is not a Host Goal catalog");
    assert!(matches!(error, CatalogError::UnknownName(name) if name == "demo"));
}

#[test]
fn unknown_name_is_rejected() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("missing", &roots).expect_err("unknown");
    assert!(matches!(error, CatalogError::UnknownName(name) if name == "missing"));
}

#[test]
fn oversized_project_file_is_not_shadowed_by_user_library() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let project_dir = project.path().join(".codex").join("workflows");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let file = File::create(project_dir.join("demo.rhai")).expect("create");
    file.set_len(MAX_WORKFLOW_SOURCE_BYTES as u64 + 1)
        .expect("sparse");
    drop(file);
    write_script(&home.path().join("workflows"), "demo", &demo_source("demo"));
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("demo", &roots).expect_err("project oversize wins");
    assert!(
        matches!(error, CatalogError::SourceLimit(_)),
        "expected SourceLimit, got {error:?}"
    );
}

#[test]
fn oversized_catalog_file_is_rejected_from_metadata_size() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let dir = home.path().join("workflows");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("huge.rhai");
    let file = File::create(&path).expect("create");
    file.set_len(MAX_WORKFLOW_SOURCE_BYTES as u64 + 1)
        .expect("sparse");
    drop(file);
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("huge", &roots).expect_err("oversize");
    match error {
        CatalogError::SourceLimit(reason) => {
            assert!(
                reason.contains("workflow source is") && reason.contains("bytes"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected SourceLimit, got {other:?}"),
    }
}

#[test]
fn overfull_catalog_scope_fails_instead_of_claiming_a_unique_name() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let dir = home.path().join("workflows");
    std::fs::create_dir_all(&dir).expect("dir");
    for index in 0..=MAX_CATALOG_SCOPE_RHAI_FILES {
        std::fs::write(dir.join(format!("n{index}.rhai")), "").expect("candidate");
    }
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("n0", &roots).expect_err("overfull");
    match error {
        CatalogError::CatalogExceeds(reason) => {
            assert!(
                reason.contains("workflow catalog exceeds"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected CatalogExceeds, got {other:?}"),
    }
}

#[test]
fn persist_json_and_scratch_dirs_are_not_catalog_entries() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let dir = home.path().join("workflows");
    write_script(&dir, "demo", &demo_source("demo"));
    std::fs::write(dir.join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.json"), "{}").expect("persist");
    std::fs::create_dir_all(dir.join("scratch")).expect("scratch");
    let roots = CatalogRoots::new(home.path(), project.path());
    let script = resolve_named("demo", &roots).expect("resolve");
    assert_eq!(script.name, "demo");

    let persist = TempDir::new().expect("persist");
    write_script(persist.path(), "demo", &demo_source("demo"));
    std::fs::write(
        persist
            .path()
            .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.json"),
        "{}",
    )
    .expect("thread json");
    std::fs::create_dir_all(persist.path().join("scratch")).expect("scratch");
    let persist_roots = CatalogRoots::from_persist_and_cwd(persist.path(), project.path());
    let persist_script = resolve_named("demo", &persist_roots).expect("persist resolve");
    assert_eq!(persist_script.name, "demo");
}

#[cfg(unix)]
#[test]
fn catalog_rejects_symlink_library_files() {
    let home = TempDir::new().expect("home");
    let project = TempDir::new().expect("project");
    let dir = home.path().join("workflows");
    std::fs::create_dir_all(&dir).expect("dir");
    let target = home.path().join("outside.rhai");
    std::fs::write(&target, demo_source("demo")).expect("target");
    std::os::unix::fs::symlink(&target, dir.join("demo.rhai")).expect("symlink");
    let roots = CatalogRoots::new(home.path(), project.path());
    let error = resolve_named("demo", &roots).expect_err("symlink");
    match error {
        CatalogError::Io { error, .. } => {
            assert!(
                error.contains("non-symlink regular file"),
                "unexpected io: {error}"
            );
        }
        other => panic!("expected Io, got {other:?}"),
    }
}
