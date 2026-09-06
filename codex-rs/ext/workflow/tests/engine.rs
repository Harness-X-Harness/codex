use pretty_assertions::assert_eq;
use rhai::Map;
use tempfile::TempDir;

use codex_workflow_extension::MAX_WORKFLOW_REPLY_CHARS;
use codex_workflow_extension::MAX_WORKFLOW_SOURCE_CHARS;
use codex_workflow_extension::MAX_WORKFLOW_YIELDS;
use codex_workflow_extension::SpawnBinding;
use codex_workflow_extension::WorkflowEval;
use codex_workflow_extension::WorkflowEvalOutcome;
use codex_workflow_extension::WorkflowSourceError;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::eval_source_with_env;
use codex_workflow_extension::eval_source_with_scratch;
use codex_workflow_extension::eval_source_with_spawn;
use codex_workflow_extension::truncate_workflow_reply;
use codex_workflow_extension::validate_source;

mod common;
use common::agent_failure_record;
use common::agent_record;
use common::ask_record;
use common::await_user_record;
use common::pause_record;
use common::spawn_failure_record;
use common::spawn_record;

#[test]
fn complete_ends_the_run() {
    assert_eq!(
        eval_source("complete();", &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn complete_stores_this_run_json_result() {
    let none = eval_source_with_env("complete();", &[], &Map::new()).expect("eval");
    assert_eq!(none.eval, WorkflowEval::Completed);
    assert_eq!(none.result, serde_json::Value::Null);

    let done = eval_source_with_env(r#"complete("done");"#, &[], &Map::new()).expect("eval");
    assert_eq!(done.eval, WorkflowEval::Completed);
    assert_eq!(done.result, serde_json::json!("done"));

    let object =
        eval_source_with_env(r#"complete(#{ ok: true });"#, &[], &Map::new()).expect("eval");
    assert_eq!(object.eval, WorkflowEval::Completed);
    assert_eq!(object.result, serde_json::json!({ "ok": true }));
}

#[test]
fn complete_rejects_other_types_and_oversize_without_completing() {
    let error = eval_source(r#"complete('x');"#, &[]).expect_err("char");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("does not accept"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }

    let mut oversized = Map::new();
    oversized.insert(
        "big".into(),
        rhai::Dynamic::from("x".repeat(MAX_WORKFLOW_SOURCE_CHARS)),
    );
    let error = eval_source_with_env("complete(args.big);", &[], &oversized)
        .expect_err("oversize complete");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(reason.contains("exceeds"), "unexpected reason: {reason}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn falling_off_the_end_completes_the_run() {
    assert_eq!(
        eval_source("let x = 1 + 1;", &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn ask_yields_then_complete_after_host_resume() {
    assert_eq!(
        eval_source(r#"ask("Compile the crate."); complete();"#, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Compile the crate.".to_string(),
        }
    );
    assert_eq!(
        eval_source(
            r#"ask("Compile the crate."); complete();"#,
            &[ask_record("Compile the crate.", "")],
        )
        .expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn agent_yields_then_branches_on_structured_result() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "ok" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Completed
    );
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "no")]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "wrong reply".to_string(),
        }
    );
}

#[test]
fn successful_empty_agent_text_is_ok() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "" && r.error == "" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn failed_agent_result_is_not_ok_and_script_can_branch() {
    let source = r#"
        let r = agent("Say ok.");
        if !r.ok && r.text == "" && r.error == "turn_errored" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_failure_record("Say ok.", "turn_errored")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn nonempty_text_is_not_the_definition_of_success() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "")]).expect("empty success"),
        WorkflowEval::Completed
    );
    assert_eq!(
        eval_source(source, &[agent_failure_record("Say ok.", "turn_errored")])
            .expect("failed nonempty would still be false"),
        WorkflowEval::Yielded {
            instruction: "wrong reply".to_string(),
        }
    );
}

#[test]
fn successful_empty_spawn_text_is_ok() {
    let source = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        if r.ok && r.text == "" && r.error == "" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    let outcome = eval_source_with_spawn(
        source,
        &[spawn_record("Say ok.", "review", "")],
        &Map::new(),
        SpawnBinding::Available,
    )
    .expect("eval");
    assert_eq!(outcome.eval, WorkflowEval::Completed);
}

#[test]
fn failed_spawn_result_replays_without_a_second_child() {
    let source = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        if !r.ok && r.error == "child_errored" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    let first =
        eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available).expect("eval");
    assert_eq!(
        first.eval,
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(first.spawn_task_name.as_deref(), Some("review"));
    let replayed = eval_source_with_spawn(
        source,
        &[spawn_failure_record("Say ok.", "review", "child_errored")],
        &Map::new(),
        SpawnBinding::Available,
    )
    .expect("replay");
    assert_eq!(replayed.eval, WorkflowEval::Completed);
    assert_eq!(replayed.spawn_task_name, None);
}

#[test]
fn agent_empty_opts_map_is_accepted() {
    let source = r#"
        let r = agent("Say ok.", #{});
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn pause_replays_completed_agent_and_then_completes() {
    let source = r#"
        let r = agent("Say ok.");
        pause();
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok"), pause_record()]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn await_user_is_a_this_run_pause() {
    assert_eq!(
        eval_source("await_user(); complete();", &[]).expect("eval"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source("await_user(); complete();", &[await_user_record()]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn write_then_read_scratch_file_completes() {
    let dir = TempDir::new().expect("tempdir");
    let source = r#"
        let name = write_scratch_file("note.txt", "hello");
        if name == "note.txt" && read_scratch_file("note.txt") == "hello" {
            complete();
        }
    "#;
    let outcome = eval_source_with_scratch(source, &[], &Map::new(), dir.path()).expect("eval");
    assert_eq!(outcome.eval, WorkflowEval::Completed);
}

#[test]
fn scratch_rejects_path_components_empty_name_and_missing_file() {
    let dir = TempDir::new().expect("tempdir");
    for source in [
        r#"write_scratch_file("../x", "no");"#,
        r#"write_scratch_file("..", "no");"#,
        r#"write_scratch_file("a/b", "no");"#,
        r#"write_scratch_file("a\\b", "no");"#,
        r#"write_scratch_file("", "no");"#,
        r#"read_scratch_file("missing.txt");"#,
    ] {
        let error =
            eval_source_with_scratch(source, &[], &Map::new(), dir.path()).expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("path component")
                        || reason.contains("must not be empty")
                        || reason.contains("not found"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn scratch_write_rejects_symlink_file() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("target");
    std::fs::write(&target, "secret").expect("target");
    std::os::unix::fs::symlink(&target, dir.path().join("note.txt")).expect("symlink");
    let error = eval_source_with_scratch(
        r#"write_scratch_file("note.txt", "hello");"#,
        &[],
        &Map::new(),
        dir.path(),
    )
    .expect_err("symlink");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(reason.contains("symlink"), "unexpected reason: {reason}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&target).expect("target"), "secret");
}

#[cfg(unix)]
#[test]
fn scratch_does_not_follow_parent_symlink() {
    let root = TempDir::new().expect("root");
    let grok = root.path().join("grok");
    std::fs::create_dir_all(&grok).expect("grok");
    let thread = root.path().join("thread");
    std::os::unix::fs::symlink(&grok, &thread).expect("symlink");
    let scratch = thread.join("scratch");
    let error = eval_source_with_scratch(
        r#"write_scratch_file("note.txt", "hello");"#,
        &[],
        &Map::new(),
        &scratch,
    )
    .expect_err("parent symlink");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(reason.contains("symlink"), "unexpected reason: {reason}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(!grok.join("scratch").join("note.txt").exists());
}

#[test]
fn scratch_read_rejects_oversized_file() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("big.txt"),
        "x".repeat(MAX_WORKFLOW_SOURCE_CHARS + 1),
    )
    .expect("seed");
    let error = eval_source_with_scratch(
        r#"read_scratch_file("big.txt");"#,
        &[],
        &Map::new(),
        dir.path(),
    )
    .expect_err("oversize");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(reason.contains("exceeds"), "unexpected reason: {reason}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn empty_agent_prompt_is_rejected() {
    let error = eval_source(r#"agent("");"#, &[]).expect_err("empty agent");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("nonempty prompt"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn yield_budget_before_any_yield_reports_the_full_allowance() {
    let source = r#"
        let b = yield_budget();
        if b.total == 32 && b.spent == 0 && b.remaining == 32 && b.reserved == () {
            complete();
        } else {
            ask("wrong budget");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn yield_budget_after_one_journaled_agent_decrements_remaining() {
    let source = r#"
        let r = agent("Say ok.");
        let b = yield_budget();
        if r.ok && b.spent == 1 && b.remaining == 31 && b.total == 32 {
            complete();
        } else {
            ask("wrong budget");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn yield_budget_branch_on_remaining_is_recomputed_on_resume() {
    let source = r#"
        let first = yield_budget();
        if first.remaining != 32 {
            ask("wrong first remaining");
        }
        agent("Say ok.");
        let second = yield_budget();
        if second.remaining == 31 {
            complete();
        } else {
            ask("wrong second remaining");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &[agent_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn empty_batch_agent_completes_without_a_yield() {
    assert_eq!(
        eval_source("batch_agent([]); complete();", &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn batch_agent_yields_each_prompt_then_branches_on_ordered_results() {
    let source = r#"
        let results = batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second" },
        ]);
        if results[0].ok && results[0].text == "one"
            && results[1].ok && results[1].text == "two"
        {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "first".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &[agent_record("first", "one")]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "second".to_string(),
        }
    );
    assert_eq!(
        eval_source(
            source,
            &[agent_record("first", "one"), agent_record("second", "two")]
        )
        .expect("eval"),
        WorkflowEval::Completed
    );
    assert_eq!(
        eval_source(
            source,
            &[agent_record("first", "one"), agent_record("second", "no")]
        )
        .expect("eval"),
        WorkflowEval::Yielded {
            instruction: "wrong reply".to_string(),
        }
    );
}

#[test]
fn batch_agent_rejects_non_map_items_and_empty_prompts() {
    for source in [
        r#"batch_agent(["x"]);"#,
        r#"batch_agent([#{ }]);"#,
        r#"batch_agent([#{ prompt: "" }]);"#,
    ] {
        let error = eval_source(source, &[]).expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("option maps")
                        || reason.contains("nonempty prompt")
                        || reason.contains("prompt"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn batch_agent_rejects_over_yield_budget_before_yielding() {
    let mut source = String::from("batch_agent([");
    for index in 0..=MAX_WORKFLOW_YIELDS {
        if index > 0 {
            source.push(',');
        }
        source.push_str(&format!("#{{ prompt: \"p{index}\" }}"));
    }
    source.push_str("]);");
    let error = eval_source(&source, &[]).expect_err("over budget");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("remaining yield budget"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn batch_agent_pause_replays_the_first_item_without_a_second_turn() {
    let source = r#"
        let results = batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second" },
        ]);
        pause();
        if results[0].text == "one" && results[1].text == "two" {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(
            source,
            &[agent_record("first", "one"), agent_record("second", "two")]
        )
        .expect("eval"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source(
            source,
            &[
                agent_record("first", "one"),
                agent_record("second", "two"),
                pause_record()
            ]
        )
        .expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn ask_returns_the_host_reply() {
    let source = r#"let x = ask("Say ok."); if x == "ok" { complete(); }"#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &[ask_record("Say ok.", "ok")]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn rhai_control_flow_is_evaluated() {
    let source = r#"
        let n = 0;
        if true {
            n = 1;
        }
        if n == 1 {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn invalid_rhai_source_is_rejected() {
    let error = validate_source("???").expect_err("invalid rhai");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("not valid Rhai"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn empty_source_is_rejected() {
    assert_eq!(validate_source("  \n"), Err(WorkflowSourceError::Empty));
}

#[test]
fn oversized_source_is_rejected() {
    let source = "x".repeat(MAX_WORKFLOW_SOURCE_CHARS + 1);
    assert_eq!(
        validate_source(&source),
        Err(WorkflowSourceError::TooLarge {
            actual: MAX_WORKFLOW_SOURCE_CHARS + 1
        })
    );
}

#[test]
fn goal_bindings_cannot_commit_goal_state() {
    for source in [
        "update_goal();",
        r#"complete_goal("done");"#,
        "block_goal();",
        "set_goal();",
        "mark_goal_complete();",
        "mark_goal_blocked();",
    ] {
        let error = eval_source(source, &[]).expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("cannot commit goal"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn truncate_workflow_reply_caps_injected_text() {
    let reply = "x".repeat(MAX_WORKFLOW_REPLY_CHARS + 8);
    assert_eq!(
        truncate_workflow_reply(&reply).chars().count(),
        MAX_WORKFLOW_REPLY_CHARS
    );
}

#[test]
fn fingerprint_is_stable_lowercase_hex_sha256() {
    let source = r#"
        let first = fingerprint("a");
        let second = fingerprint("a");
        let other = fingerprint("b");
        let empty = fingerprint("");
        if first == second
            && first != other
            && first == "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
            && other == "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d"
            && empty == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        {
            complete();
        } else {
            ask("wrong fingerprint");
        }
    "#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn json_encode_covers_accepted_types_and_rejects_others() {
    let accepted = r#"
        if json_encode(()) == "null"
            && json_encode(true) == "true"
            && json_encode(1) == "1"
            && json_encode(1.5) == "1.5"
            && json_encode("x") == "\"x\""
            && json_encode([1, "x"]) == "[1,\"x\"]"
            && json_encode(#{ ok: true }) == "{\"ok\":true}"
        {
            complete();
        } else {
            ask("wrong json");
        }
    "#;
    assert_eq!(
        eval_source(accepted, &[]).expect("eval"),
        WorkflowEval::Completed
    );

    let object = eval_source(
        r#"
            let text = json_encode(#{ ok: true, n: 1 });
            ask(text);
        "#,
        &[],
    )
    .expect("eval");
    match object {
        WorkflowEval::Yielded { instruction } => {
            let value: serde_json::Value = serde_json::from_str(&instruction).expect("object json");
            assert_eq!(
                value,
                serde_json::json!({
                    "ok": true,
                    "n": 1
                })
            );
        }
        other => panic!("expected Yielded object text, got {other:?}"),
    }

    let error = eval_source(r#"json_encode('x');"#, &[]).expect_err("char");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("does not accept"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }

    let mut oversized = Map::new();
    oversized.insert(
        "big".into(),
        rhai::Dynamic::from("x".repeat(MAX_WORKFLOW_SOURCE_CHARS)),
    );
    let error =
        eval_source_with_env("json_encode(args.big);", &[], &oversized).expect_err("oversize json");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(reason.contains("exceeds"), "unexpected reason: {reason}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn script_branches_on_fingerprint_and_json_encode() {
    let source = r#"
        if fingerprint(args.text) == fingerprint("a")
            && json_encode(()) == "null"
        {
            complete();
        } else {
            ask("wrong helper result");
        }
    "#;
    let mut matching = Map::new();
    matching.insert("text".into(), rhai::Dynamic::from("a"));
    assert_eq!(
        eval_source_with_env(source, &[], &matching)
            .expect("eval")
            .eval,
        WorkflowEval::Completed
    );

    let mut other = Map::new();
    other.insert("text".into(), rhai::Dynamic::from("b"));
    assert_eq!(
        eval_source_with_env(source, &[], &other)
            .expect("eval")
            .eval,
        WorkflowEval::Yielded {
            instruction: "wrong helper result".to_string(),
        }
    );
}

#[test]
fn named_program_reads_args_and_records_phase() {
    let source = r#"
        let meta = #{
            name: "demo",
            description: "named",
        };
        phase("Scan");
        if args.topic == "rust" {
            complete();
        } else {
            ask("wrong args");
        }
    "#;
    let mut args = Map::new();
    args.insert("topic".into(), rhai::Dynamic::from("rust"));
    let outcome = eval_source_with_env(source, &[], &args).expect("eval");
    assert_eq!(
        outcome,
        WorkflowEvalOutcome {
            eval: WorkflowEval::Completed,
            phase: Some("Scan".to_string()),
            log: None,
            result: serde_json::Value::Null,
            spawn_task_name: None,
            yield_kind: None,
            yield_request_digest: None,
        }
    );
}

#[test]
fn log_records_last_nonempty_message_and_rejects_empty() {
    let outcome = eval_source_with_env(
        r#"log("first"); log("note"); complete();"#,
        &[],
        &Map::new(),
    )
    .expect("eval");
    assert_eq!(
        outcome,
        WorkflowEvalOutcome {
            eval: WorkflowEval::Completed,
            phase: None,
            log: Some("note".to_string()),
            result: serde_json::Value::Null,
            spawn_task_name: None,
            yield_kind: None,
            yield_request_digest: None,
        }
    );

    for source in [r#"log("");"#, r#"log("   ");"#] {
        let error = eval_source(source, &[]).expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("nonempty message"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn spawn_request_without_task_name_is_rejected_before_a_yield() {
    for source in [
        r#"agent("Say ok.", #{ "spawn": true });"#,
        r#"agent("Say ok.", #{ "spawn": true, task_name: "" });"#,
        r#"agent("Say ok.", #{ "spawn": true, task_name: "   " });"#,
        r#"batch_agent([#{ prompt: "Say ok.", "spawn": true }]);"#,
    ] {
        let error = eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available)
            .expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("task_name"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn unavailable_spawn_is_rejected_before_a_yield() {
    let source = r#"agent("Say ok.", #{ "spawn": true, task_name: "review" });"#;
    let error = eval_source(source, &[]).expect_err("unavailable spawn");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("unavailable"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn spawn_false_without_task_name_stays_same_thread() {
    let source = r#"let r = agent("Say ok.", #{ "spawn": false }); if r.ok { complete(); }"#;
    let outcome =
        eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available).expect("eval");
    assert_eq!(
        outcome.eval,
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(outcome.spawn_task_name, None);
}

#[test]
fn available_spawn_yields_then_replays_without_a_second_child() {
    let source = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        if r.ok && r.text == "ok" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    let first =
        eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available).expect("eval");
    assert_eq!(
        first.eval,
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(first.spawn_task_name.as_deref(), Some("review"));
    let replayed = eval_source_with_spawn(
        source,
        &[spawn_record("Say ok.", "review", "ok")],
        &Map::new(),
        SpawnBinding::Available,
    )
    .expect("replay");
    assert_eq!(replayed.eval, WorkflowEval::Completed);
    assert_eq!(replayed.spawn_task_name, None);
}

#[test]
fn batch_agent_items_can_request_spawn_independently() {
    let source = r#"
        let results = batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second", "spawn": true, task_name: "review" },
        ]);
        if results[0].text == "one" && results[1].text == "two" {
            complete();
        }
    "#;
    let first =
        eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available).expect("eval");
    assert_eq!(
        first.eval,
        WorkflowEval::Yielded {
            instruction: "first".to_string(),
        }
    );
    assert_eq!(first.spawn_task_name, None);
    let second = eval_source_with_spawn(
        source,
        &[agent_record("first", "one")],
        &Map::new(),
        SpawnBinding::Available,
    )
    .expect("second");
    assert_eq!(
        second.eval,
        WorkflowEval::Yielded {
            instruction: "second".to_string(),
        }
    );
    assert_eq!(second.spawn_task_name.as_deref(), Some("review"));
}

#[test]
fn unknown_agent_option_keys_fail_before_a_yield() {
    for source in [
        r#"agent("Say ok.", #{ model: "x" });"#,
        r#"agent("Say ok.", #{ effort: "high" });"#,
        r#"agent("Say ok.", #{ isolation_worktree: true });"#,
        r#"agent("Say ok.", #{ fork_context: true });"#,
        r#"agent("Say ok.", #{ resume_from: "id" });"#,
        r#"agent("Say ok.", #{ output_schema: #{} });"#,
        r#"agent("Say ok.", #{ label: "unused" });"#,
    ] {
        let error = eval_source(source, &[]).expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("does not accept option"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn unknown_batch_agent_item_key_fails_before_any_host_work() {
    let source = r#"
        batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second", model: "x" },
        ]);
    "#;
    let error = eval_source(source, &[]).expect_err("unknown item key");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("does not accept option"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn task_name_without_spawn_fails_before_a_yield() {
    for source in [
        r#"agent("Say ok.", #{ task_name: "review" });"#,
        r#"agent("Say ok.", #{ "spawn": false, task_name: "review" });"#,
        r#"batch_agent([#{ prompt: "Say ok.", task_name: "review" }]);"#,
    ] {
        let error = eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available)
            .expect_err(source);
        match error {
            WorkflowSourceError::Invalid { reason } => {
                assert!(
                    reason.contains("task_name") && reason.contains("spawn"),
                    "{source} unexpected reason: {reason}"
                );
            }
            other => panic!("{source} expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn spawn_option_must_be_boolean() {
    let source = r#"agent("Say ok.", #{ "spawn": "true", task_name: "review" });"#;
    let error = eval_source_with_spawn(source, &[], &Map::new(), SpawnBinding::Available)
        .expect_err("spawn string");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("spawn") && reason.contains("boolean"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn old_parallel_and_budget_names_fail_with_migration_messages() {
    let parallel = eval_source(r#"parallel([#{ prompt: "first" }]);"#, &[]).expect_err("parallel");
    match parallel {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("renamed to batch_agent()"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }

    let budget = eval_source("budget();", &[]).expect_err("budget");
    match budget {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("renamed to yield_budget()"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn max_operations_stops_unbounded_work() {
    let error = eval_source("loop { }", &[]).expect_err("loop");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.to_ascii_lowercase().contains("operation")
                    || reason.to_ascii_lowercase().contains("limit"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}
