use pretty_assertions::assert_eq;
use rhai::Map;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::ContinuationKind;
use codex_workflow_extension::ContinuationRecord;
use codex_workflow_extension::HostCallResult;
use codex_workflow_extension::MAX_WORKFLOW_CONTROL_RESUMES;
use codex_workflow_extension::MAX_WORKFLOW_YIELDS;
use codex_workflow_extension::REPLAY_DIVERGENCE;
use codex_workflow_extension::SpawnBinding;
use codex_workflow_extension::WorkflowAdvance;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::eval_source;
use codex_workflow_extension::eval_source_with_spawn;

mod common;
use common::agent_record;
use common::pause_record;

#[test]
fn same_ask_replays_without_a_second_host_turn() {
    let source = r#"ask("Compile the crate."); complete();"#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(1), source).expect("start");
    assert_eq!(
        run.advance_with_reply("compiled".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.continuations.len(), 1);
    assert_eq!(run.continuations[0].kind, ContinuationKind::Ask);
}

#[test]
fn same_agent_replays_without_a_second_host_turn() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "ok" { complete(); }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(2), source).expect("start");
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.continuations[0].kind, ContinuationKind::Agent);
}

#[test]
fn same_spawn_replays_without_a_second_child() {
    let source = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        if r.ok && r.text == "ok" { complete(); }
    "#;
    let mut run =
        WorkflowRun::start_with_spawn(ThreadId::from_u128(3), source, SpawnBinding::Available)
            .expect("start");
    assert_eq!(run.pending_spawn_task_name.as_deref(), Some("review"));
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.continuations[0].kind, ContinuationKind::SpawnAgent);
    assert_eq!(run.pending_spawn_task_name, None);
}

#[test]
fn scratch_changing_completed_agent_prompt_fails_closed() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("p.txt"), "one").expect("seed");
    let source = r#"
        let r = agent(read_scratch_file("p.txt"));
        pause();
        if r.text == "ok" { complete(); }
    "#;
    let mut run =
        WorkflowRun::start_with_scratch(ThreadId::from_u128(4), source, dir.path().to_path_buf())
            .expect("start");
    assert_eq!(run.pending_instruction.as_deref(), Some("one"));
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Paused)
    );
    std::fs::write(dir.path().join("p.txt"), "two").expect("change");
    run.resume().expect("failed closed");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("replay_diverged"));
    assert!(!run.occupies_idle());
    assert_eq!(run.pending_instruction, None);
    assert_eq!(run.continuations.len(), 1);
}

#[test]
fn spawn_task_name_change_fails_before_a_child_starts() {
    let first = r#"agent("Say ok.", #{ "spawn": true, task_name: "review" }); complete();"#;
    let mut run =
        WorkflowRun::start_with_spawn(ThreadId::from_u128(5), first, SpawnBinding::Available)
            .expect("start");
    run.advance_with_reply("ok".to_string()).expect("reply");
    let changed = r#"agent("Say ok.", #{ "spawn": true, task_name: "other" }); complete();"#;
    let error = eval_source_with_spawn(
        changed,
        &run.continuations,
        &Map::new(),
        SpawnBinding::Available,
    )
    .expect_err("diverged");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}

#[test]
fn ask_to_agent_kind_change_fails_closed() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(6), r#"ask("Say ok."); complete();"#)
        .expect("start");
    run.advance_with_reply("ok".to_string()).expect("reply");
    let error =
        eval_source(r#"agent("Say ok."); complete();"#, &run.continuations).expect_err("kind");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}

#[test]
fn pause_cannot_consume_an_await_user_slot() {
    let mut run =
        WorkflowRun::start(ThreadId::from_u128(7), "await_user(); complete();").expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.pending_kind, Some(ContinuationKind::AwaitUser));
    run.resume().expect("resume await_user");
    assert_eq!(run.status, WorkflowStatus::Complete);
    let error = eval_source("pause(); complete();", &run.continuations).expect_err("kind");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}

#[test]
fn legacy_positional_active_run_is_rejected() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(8), "complete();").expect("start");
    run.status = WorkflowStatus::Active;
    run.format_version = 1;
    run.served_replies = vec!["ok".to_string()];
    run.continuations.clear();
    run.prepare_restored().expect("legacy");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("legacy_resume_required"));
    assert!(!run.occupies_idle());
}

#[test]
fn batch_agent_item_identity_change_fails_closed() {
    let first = r#"
        let results = batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second" },
        ]);
        complete();
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(10), first).expect("start");
    run.advance_with_reply("one".to_string()).expect("first");
    run.advance_with_reply("two".to_string()).expect("second");
    let changed = r#"
        let results = batch_agent([
            #{ prompt: "other" },
            #{ prompt: "second" },
        ]);
        complete();
    "#;
    let error = eval_source(changed, &run.continuations).expect_err("diverged");
    assert!(
        error.to_string().contains(REPLAY_DIVERGENCE),
        "unexpected error: {error}"
    );
}

#[test]
fn malformed_journal_sequence_is_rejected() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(9), "complete();").expect("start");
    run.continuations.push(ContinuationRecord {
        seq: 9,
        kind: ContinuationKind::Ask,
        request_digest: "nonzero".to_string(),
        result: HostCallResult::success("y"),
    });
    run.prepare_restored().expect("seq");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("unsafe_journal"));
    assert!(!run.occupies_idle());
}

#[test]
fn persisted_string_result_replays_as_successful_text() {
    let record: ContinuationRecord = serde_json::from_value(serde_json::json!({
        "seq": 1,
        "kind": "agent",
        "request_digest": "abc",
        "result": "ok"
    }))
    .expect("legacy string");
    assert_eq!(record.result, HostCallResult::success("ok"));
}

#[test]
fn restore_rejects_too_many_result_bearing_records() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(11), "complete();").expect("start");
    let records = (0..=MAX_WORKFLOW_YIELDS)
        .map(|index| agent_record(&format!("p{index}"), "ok"))
        .collect();
    run.continuations = with_dense_seq(records);
    run.prepare_restored().expect("restore");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("unsafe_journal"));
    assert!(!run.occupies_idle());
}

#[test]
fn restore_rejects_too_many_control_resumes() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(12), "complete();").expect("start");
    let records = (0..=MAX_WORKFLOW_CONTROL_RESUMES)
        .map(|index| pause_record(&format!("{}:1", index.saturating_add(1))))
        .collect();
    run.continuations = with_dense_seq(records);
    run.prepare_restored().expect("restore");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("unsafe_journal"));
    assert!(!run.occupies_idle());
}

#[test]
fn restore_accepts_full_mixed_host_and_control_allowance() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(13), "complete();").expect("start");
    let mut records = Vec::new();
    records.extend(
        (0..MAX_WORKFLOW_CONTROL_RESUMES)
            .map(|index| pause_record(&format!("{}:1", index.saturating_add(1)))),
    );
    records.extend((0..MAX_WORKFLOW_YIELDS).map(|index| agent_record(&format!("p{index}"), "ok")));
    run.continuations = with_dense_seq(records);
    run.prepare_restored().expect("restore");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.served_asks, MAX_WORKFLOW_YIELDS);
    assert_eq!(
        run.continuations.len(),
        (MAX_WORKFLOW_YIELDS + MAX_WORKFLOW_CONTROL_RESUMES) as usize
    );
}

fn with_dense_seq(mut records: Vec<ContinuationRecord>) -> Vec<ContinuationRecord> {
    for (index, record) in records.iter_mut().enumerate() {
        record.seq = u32::try_from(index.saturating_add(1)).expect("seq");
    }
    records
}
