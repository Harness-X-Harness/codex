use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_workflow_extension::CatalogError;
use codex_workflow_extension::CatalogRoots;
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
