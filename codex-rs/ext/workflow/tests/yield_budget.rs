use pretty_assertions::assert_eq;
use rhai::Map;

use codex_protocol::ThreadId;
use codex_workflow_extension::MAX_WORKFLOW_CONTROL_RESUMES;
use codex_workflow_extension::MAX_WORKFLOW_YIELDS;
use codex_workflow_extension::SpawnBinding;
use codex_workflow_extension::WorkflowAdvance;
use codex_workflow_extension::WorkflowEval;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowSourceError;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::eval_source_with_spawn;

mod common;
use common::agent_record;
use common::await_user_record;
use common::pause_record;
use common::source_callsite;

#[test]
fn yield_budget_ignores_completed_control_resumes() {
    let mut source = String::new();
    let mut journal = Vec::new();
    for _ in 0..MAX_WORKFLOW_CONTROL_RESUMES {
        source.push_str("pause();\n");
        journal.push(pause_record(&source_callsite(
            &source,
            "pause",
            source.matches("pause(").count() - 1,
        )));
    }
    source.push_str(
        r#"
        let r = agent("Say ok.");
        let b = yield_budget();
        if r.ok && b.spent == 1 && b.remaining == 31 && b.total == 32 {
            complete();
        } else {
            ask("wrong budget");
        }
    "#,
    );
    journal.push(agent_record("Say ok.", "ok"));
    assert_eq!(
        eval_source(&source, &journal).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn full_control_and_result_bearing_allowances_coexist() {
    let (mut source, journal) =
        mixed_pause_and_agent_program(MAX_WORKFLOW_CONTROL_RESUMES, MAX_WORKFLOW_YIELDS);
    source.push_str(
        r#"
        let b = yield_budget();
        if b.spent == 32 && b.remaining == 0 && b.total == 32 {
            complete();
        } else {
            ask("wrong budget");
        }
    "#,
    );
    assert_eq!(
        eval_source(&source, &journal).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn mixed_pause_and_await_user_fill_the_control_allowance() {
    let mut source = String::new();
    let mut journal = Vec::new();
    for _ in 0..(MAX_WORKFLOW_CONTROL_RESUMES / 2) {
        source.push_str("pause();\n");
        journal.push(pause_record(&source_callsite(
            &source,
            "pause",
            source.matches("pause(").count() - 1,
        )));
        source.push_str("await_user();\n");
        journal.push(await_user_record(&source_callsite(
            &source,
            "await_user",
            source.matches("await_user(").count() - 1,
        )));
    }
    for index in 0..MAX_WORKFLOW_YIELDS {
        let prompt = format!("p{index}");
        source.push_str(&format!("agent(\"{prompt}\");\n"));
        journal.push(agent_record(&prompt, "ok"));
    }
    source.push_str(
        r#"
        let b = yield_budget();
        if b.spent == 32 && b.remaining == 0 {
            complete();
        } else {
            ask("wrong budget");
        }
    "#,
    );
    assert_eq!(
        eval_source(&source, &journal).expect("eval"),
        WorkflowEval::Completed
    );
}

#[test]
fn extra_result_bearing_yield_fails_before_host_work() {
    let (mut source, journal) =
        mixed_pause_and_agent_program(MAX_WORKFLOW_CONTROL_RESUMES, MAX_WORKFLOW_YIELDS);
    source.push_str(r#"agent("overflow"); complete();"#);
    let error = eval_source(&source, &journal).expect_err("over yield");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("exceeded") && reason.contains("yields"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn extra_spawn_fails_before_a_child_starts() {
    let (mut source, journal) = mixed_pause_and_agent_program(0, MAX_WORKFLOW_YIELDS);
    source.push_str(r#"agent("overflow", #{ "spawn": true, task_name: "review" });"#);
    let error = eval_source_with_spawn(&source, &journal, &Map::new(), SpawnBinding::Available)
        .expect_err("over yield");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("exceeded") && reason.contains("yields"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn extra_control_resume_fails_without_spending_result_bearing() {
    let (mut source, journal) = mixed_pause_and_agent_program(MAX_WORKFLOW_CONTROL_RESUMES, 1);
    source.push_str("pause(); complete();");
    let error = eval_source(&source, &journal).expect_err("over control");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("exceeded") && reason.contains("control resumes"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }

    let mut run_source = String::from(r#"agent("one");"#);
    for _ in 0..=MAX_WORKFLOW_CONTROL_RESUMES {
        run_source.push_str("pause();");
    }
    run_source.push_str("complete();");
    let mut run = WorkflowRun::start(ThreadId::from_u128(21), &run_source).expect("start");
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Paused)
    );
    let mut resumes = 0u32;
    while run.status == WorkflowStatus::Paused && resumes < 40 {
        run.resume().expect("resume");
        resumes += 1;
    }
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.served_asks, 1);
    assert!(resumes <= MAX_WORKFLOW_CONTROL_RESUMES.saturating_add(1));
}

#[test]
fn extra_await_user_fails_at_the_control_boundary() {
    let mut source = String::new();
    let mut journal = Vec::new();
    source.push_str(r#"agent("one");"#);
    journal.push(agent_record("one", "ok"));
    for _ in 0..MAX_WORKFLOW_CONTROL_RESUMES {
        source.push_str("await_user();\n");
        journal.push(await_user_record(&source_callsite(
            &source,
            "await_user",
            source.matches("await_user(").count() - 1,
        )));
    }
    source.push_str("await_user(); complete();");
    let error = eval_source(&source, &journal).expect_err("over await_user");
    match error {
        WorkflowSourceError::Invalid { reason } => {
            assert!(
                reason.contains("exceeded") && reason.contains("control resumes"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn batch_agent_uses_result_bearing_remaining_after_controls() {
    let mut source = String::new();
    let mut journal = Vec::new();
    for _ in 0..MAX_WORKFLOW_CONTROL_RESUMES {
        source.push_str("pause();\n");
        journal.push(pause_record(&source_callsite(
            &source,
            "pause",
            source.matches("pause(").count() - 1,
        )));
    }
    for index in 0..(MAX_WORKFLOW_YIELDS - 1) {
        let prompt = format!("p{index}");
        source.push_str(&format!("agent(\"{prompt}\");\n"));
        journal.push(agent_record(&prompt, "ok"));
    }
    source.push_str(
        r#"
        batch_agent([
            #{ prompt: "a" },
            #{ prompt: "b" },
        ]);
    "#,
    );
    let error = eval_source(&source, &journal).expect_err("batch over remaining");
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

fn mixed_pause_and_agent_program(
    pauses: u32,
    agents: u32,
) -> (String, Vec<codex_workflow_extension::ContinuationRecord>) {
    let mut source = String::new();
    let mut journal = Vec::new();
    for _ in 0..pauses {
        source.push_str("pause();\n");
        journal.push(pause_record(&source_callsite(
            &source,
            "pause",
            source.matches("pause(").count() - 1,
        )));
    }
    for index in 0..agents {
        let prompt = format!("p{index}");
        source.push_str(&format!("agent(\"{prompt}\");\n"));
        journal.push(agent_record(&prompt, "ok"));
    }
    (source, journal)
}
