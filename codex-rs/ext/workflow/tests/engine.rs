use pretty_assertions::assert_eq;

use codex_workflow_extension::MAX_WORKFLOW_REPLY_CHARS;
use codex_workflow_extension::MAX_WORKFLOW_SOURCE_CHARS;
use codex_workflow_extension::WorkflowEval;
use codex_workflow_extension::WorkflowSourceError;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::eval_source_with_pauses;
use codex_workflow_extension::truncate_workflow_reply;
use codex_workflow_extension::validate_source;

#[test]
fn complete_ends_the_run() {
    assert_eq!(
        eval_source("complete();", &[]).expect("eval"),
        WorkflowEval::Completed
    );
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
            &[String::new()]
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
        eval_source(source, &["ok".to_string()]).expect("eval"),
        WorkflowEval::Completed
    );
    assert_eq!(
        eval_source(source, &["no".to_string()]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "wrong reply".to_string(),
        }
    );
}

#[test]
fn agent_opts_map_is_accepted() {
    let source = r#"
        let r = agent("Say ok.", #{ label: "w1" });
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    assert_eq!(
        eval_source(source, &["ok".to_string()]).expect("eval"),
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
        eval_source(source, &["ok".to_string()]).expect("eval"),
        WorkflowEval::Paused
    );
    assert_eq!(
        eval_source_with_pauses(source, &["ok".to_string()], 1).expect("eval"),
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
        eval_source_with_pauses("await_user(); complete();", &[], 1).expect("eval"),
        WorkflowEval::Completed
    );
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
fn ask_returns_the_host_reply() {
    let source = r#"let x = ask("Say ok."); if x == "ok" { complete(); }"#;
    assert_eq!(
        eval_source(source, &[]).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Say ok.".to_string(),
        }
    );
    assert_eq!(
        eval_source(source, &["ok".to_string()]).expect("eval"),
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
