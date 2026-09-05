use pretty_assertions::assert_eq;

use codex_workflow_extension::MAX_WORKFLOW_SOURCE_CHARS;
use codex_workflow_extension::WorkflowEval;
use codex_workflow_extension::WorkflowSourceError;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::validate_source;

#[test]
fn complete_ends_the_run() {
    assert_eq!(
        eval_source("complete();", 0).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn falling_off_the_end_completes_the_run() {
    assert_eq!(
        eval_source("let x = 1 + 1;", 0).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn ask_yields_then_complete_after_host_resume() {
    assert_eq!(
        eval_source(r#"ask("Compile the crate."); complete();"#, 0).expect("eval"),
        WorkflowEval::Yielded {
            instruction: "Compile the crate.".to_string(),
        }
    );
    assert_eq!(
        eval_source(r#"ask("Compile the crate."); complete();"#, 1).expect("eval"),
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
        eval_source(source, 0).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn markdown_step_table_is_rejected() {
    let error = validate_source("# Ship\n\n## Build\nCompile the crate.\n").expect_err("markdown");
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
        let error = eval_source(source, 0).expect_err(source);
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
fn max_operations_stops_unbounded_work() {
    let error = eval_source("loop { }", 0).expect_err("loop");
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
