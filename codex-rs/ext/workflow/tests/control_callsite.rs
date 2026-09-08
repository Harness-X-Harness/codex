use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::REPLAY_DIVERGENCE;
use codex_workflow_extension::SpawnBinding;
use codex_workflow_extension::WorkflowEval;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::eval_source_with_spawn;

mod common;
use common::agent_record;
use common::await_user_record;
use common::pause_record;
use common::source_callsite;
use common::spawn_record;

#[test]
fn pause_resumes_through_the_same_callsite() {
    let source = "pause(); complete();";
    assert_eq!(
        eval_source(source, &[]).expect("pause"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source(
            source,
            &[pause_record(&source_callsite(source, "pause", 0))]
        )
        .expect("resume"),
        WorkflowEval::Completed
    );
}

#[test]
fn await_user_resumes_through_the_same_callsite() {
    let source = "await_user(); complete();";
    assert_eq!(
        eval_source(source, &[]).expect("await"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source(
            source,
            &[await_user_record(&source_callsite(source, "await_user", 0))]
        )
        .expect("resume"),
        WorkflowEval::Completed
    );
}

#[test]
fn switching_pause_callsites_at_the_same_seq_diverges() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("mode"), "a").expect("seed");
    let source = r#"
        if read_scratch_file("mode") == "a" {
            pause();
        } else {
            pause();
        }
        complete();
    "#;
    let mut run =
        WorkflowRun::start_with_scratch(ThreadId::from_u128(31), source, dir.path().to_path_buf())
            .expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    std::fs::write(dir.path().join("mode"), "b").expect("switch");
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("replay_diverged"));
    assert!(!run.occupies_idle());
}

#[test]
fn switching_await_user_callsites_at_the_same_seq_diverges() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("mode"), "a").expect("seed");
    let source = r#"
        if read_scratch_file("mode") == "a" {
            await_user();
        } else {
            await_user();
        }
        complete();
    "#;
    let mut run =
        WorkflowRun::start_with_scratch(ThreadId::from_u128(32), source, dir.path().to_path_buf())
            .expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    std::fs::write(dir.path().join("mode"), "b").expect("switch");
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("replay_diverged"));
    assert!(!run.occupies_idle());
}

#[test]
fn pause_versus_await_user_still_diverges() {
    let source = "await_user(); complete();";
    let error = eval_source(
        "pause(); complete();",
        &[await_user_record(&source_callsite(source, "await_user", 0))],
    )
    .expect_err("kind");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}

#[test]
fn control_callsite_survives_serialize_and_recompile() {
    let source = "pause(); complete();";
    let mut run = WorkflowRun::start(ThreadId::from_u128(33), source).expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    let body = serde_json::to_string(&run).expect("serialize");
    let mut restored: WorkflowRun = serde_json::from_str(&body).expect("deserialize");
    restored.prepare_restored().expect("restore");
    restored.resume().expect("resume");
    assert_eq!(restored.status, WorkflowStatus::Complete);
}

#[test]
fn result_bearing_ask_and_agent_identity_is_unchanged() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("agent"),
        WorkflowEval::Completed
    );
    let asked = r#"ask("Compile the crate."); complete();"#;
    assert_eq!(
        eval_source(asked, &[]).expect("ask"),
        WorkflowEval::Yielded {
            instruction: "Compile the crate.".to_string(),
        }
    );
    let spawned = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        if r.ok && r.text == "ok" { complete(); }
    "#;
    assert_eq!(
        eval_source_with_spawn(
            spawned,
            &[spawn_record("Say ok.", "review", "ok")],
            &rhai::Map::new(),
            SpawnBinding::Available,
        )
        .expect("spawn")
        .eval,
        WorkflowEval::Completed
    );
}

#[test]
fn two_pause_callsites_do_not_share_a_digest() {
    let source = r#"
        pause();
        pause();
        complete();
    "#;
    let first = source_callsite(source, "pause", 0);
    let second = source_callsite(source, "pause", 1);
    assert_eq!(
        eval_source(source, &[pause_record(&first)]).expect("second pause"),
        WorkflowEval::Paused
    );
    let error = eval_source(source, &[pause_record(&second)]).expect_err("wrong first callsite");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}
