use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_protocol::ThreadId;
use codex_workflow_extension::ContinuationKind;
use codex_workflow_extension::HostCallResult;
use codex_workflow_extension::SpawnBinding;
use codex_workflow_extension::WorkflowAdvance;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;

fn result_replies(run: &WorkflowRun) -> Vec<String> {
    run.continuations
        .iter()
        .filter(|record| {
            matches!(
                record.kind,
                ContinuationKind::Ask | ContinuationKind::Agent | ContinuationKind::SpawnAgent
            )
        })
        .map(|record| record.result.text.clone())
        .collect()
}

fn yield_then_complete() -> &'static str {
    r#"ask("Compile the crate."); complete();"#
}

#[test]
fn start_complete_without_yield() {
    let run = WorkflowRun::start(ThreadId::from_u128(1), "complete();").expect("start");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.pending_instruction, None);
    assert_eq!(run.result, serde_json::Value::Null);
}

#[test]
fn complete_value_persists_on_the_run() {
    let run = WorkflowRun::start(ThreadId::from_u128(21), r#"complete("done");"#).expect("start");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.result, serde_json::json!("done"));
}

#[test]
fn rejected_complete_value_does_not_create_a_run() {
    let error = WorkflowRun::start(ThreadId::from_u128(22), r#"complete('x');"#)
        .expect_err("rejected complete");
    assert!(
        error.contains("does not accept"),
        "unexpected error: {error}"
    );
}

#[test]
fn advance_resumes_after_agent_then_branches() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok && r.text == "ok" {
            complete();
        } else {
            ask("wrong reply");
        }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(9), source).expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("Say ok."));
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);

    let mut missed = WorkflowRun::start(ThreadId::from_u128(10), source).expect("start");
    assert_eq!(
        missed.advance_with_reply("no".to_string()),
        Ok(WorkflowAdvance::Yielded)
    );
    assert_eq!(missed.pending_instruction.as_deref(), Some("wrong reply"));
}

#[test]
fn scratch_survives_pause_and_resume() {
    let dir = TempDir::new().expect("tempdir");
    let source = r#"
        write_scratch_file("note.txt", "hello");
        pause();
        if read_scratch_file("note.txt") == "hello" {
            complete();
        }
    "#;
    let mut run =
        WorkflowRun::start_with_scratch(ThreadId::from_u128(15), source, dir.path().to_path_buf())
            .expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).expect("persisted"),
        "hello"
    );
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Complete);
}

#[test]
fn log_survives_pause_and_resume_without_a_yield() {
    let source = r#"
        log("note");
        pause();
        complete();
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(18), source).expect("start");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.log.as_deref(), Some("note"));
    assert_eq!(run.served_asks, 0);
    assert!(run.continuations.is_empty());
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.log.as_deref(), Some("note"));
    assert_eq!(run.served_asks, 0);
}

#[test]
fn scratch_survives_stop_and_resume() {
    let dir = TempDir::new().expect("tempdir");
    let source = r#"
        write_scratch_file("note.txt", "hello");
        ask("continue");
        if read_scratch_file("note.txt") == "hello" {
            complete();
        }
    "#;
    let mut run =
        WorkflowRun::start_with_scratch(ThreadId::from_u128(17), source, dir.path().to_path_buf())
            .expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).expect("persisted"),
        "hello"
    );
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).expect("after stop"),
        "hello"
    );
    run.resume().expect("resume");
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
}

#[test]
fn batch_agent_stop_after_first_item_replays_without_a_second_turn() {
    let source = r#"
        let results = batch_agent([
            #{ prompt: "first" },
            #{ prompt: "second" },
        ]);
        if results[0].text == "one" && results[1].text == "two" {
            complete();
        }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(14), source).expect("start");
    assert_eq!(run.pending_instruction.as_deref(), Some("first"));
    assert_eq!(
        run.advance_with_reply("one".to_string()),
        Ok(WorkflowAdvance::Yielded)
    );
    assert_eq!(run.pending_instruction.as_deref(), Some("second"));
    assert_eq!(result_replies(&run), vec!["one".to_string()]);
    run.stop().expect("stop");
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(run.pending_instruction.as_deref(), Some("second"));
    assert_eq!(result_replies(&run), vec!["one".to_string()]);
    assert_eq!(
        run.advance_with_reply("two".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(
        result_replies(&run),
        vec!["one".to_string(), "two".to_string()]
    );
}

#[test]
fn agent_then_pause_resumes_without_a_second_host_turn() {
    let source = r#"
        let r = agent("Say ok.");
        pause();
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(11), source).expect("start");
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Paused)
    );
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(result_replies(&run), vec!["ok".to_string()]);
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert!(
        run.continuations
            .iter()
            .any(|record| record.kind == ContinuationKind::Pause)
    );
    assert_eq!(result_replies(&run), vec!["ok".to_string()]);
}

#[test]
fn advance_resumes_after_ask_then_completes() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(2), yield_then_complete()).expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
    assert!(!run.pending_yield_started);
    run.mark_pending_yield_started();
    assert!(run.pending_yield_started);
    assert_eq!(
        run.advance_with_reply("compiled".to_string()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(result_replies(&run), vec!["compiled".to_string()]);
    assert!(!run.occupies_idle());
}

#[test]
fn active_run_occupies_idle() {
    let run = WorkflowRun::start(ThreadId::from_u128(8), yield_then_complete()).expect("start");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert!(run.occupies_idle());
}

#[test]
fn queued_run_does_not_occupy_until_activated() {
    let mut run =
        WorkflowRun::queue(ThreadId::from_u128(11), yield_then_complete()).expect("queue");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    assert!(!run.occupies_idle());
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Yielded));
    assert_eq!(run.status, WorkflowStatus::Active);
    assert!(run.occupies_idle());
}

#[test]
fn queued_named_run_keeps_args_until_activated() {
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
    let mut args = serde_json::Map::new();
    args.insert("topic".into(), serde_json::Value::String("rust".into()));
    let mut run = WorkflowRun::queue_named(ThreadId::from_u128(13), "demo", source, args)
        .expect("queue named");
    assert_eq!(run.name, "demo");
    assert_eq!(run.status, WorkflowStatus::Waiting);
    assert_eq!(run.phase, None);
    assert!(!run.occupies_idle());
    assert_eq!(run.activate(), Ok(WorkflowAdvance::Completed));
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.phase.as_deref(), Some("Scan"));
}

#[test]
fn stop_can_cancel_a_waiting_run() {
    let mut run =
        WorkflowRun::queue(ThreadId::from_u128(12), yield_then_complete()).expect("queue");
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    run.park().expect("park");
    assert_eq!(run.status, WorkflowStatus::Waiting);
}

#[test]
fn stop_and_resume_are_host_owned() {
    let mut run = WorkflowRun::start(ThreadId::from_u128(3), yield_then_complete()).expect("start");
    run.mark_pending_yield_started();
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert!(!run.pending_yield_started);
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Active);
    assert_eq!(
        run.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
}

#[tokio::test]
async fn service_scratch_lives_under_the_thread_dir() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(16);
    let started = service
        .start_run(
            thread_id,
            r#"
                write_scratch_file("note.txt", "hello");
                if read_scratch_file("note.txt") == "hello" {
                    complete();
                }
            "#,
        )
        .await
        .expect("start");
    assert_eq!(started.status, WorkflowStatus::Complete);
    let stored = std::fs::read_to_string(
        dir.path()
            .join(thread_id.to_string())
            .join("scratch")
            .join("note.txt"),
    )
    .expect("scratch file");
    assert_eq!(stored, "hello");
}

#[tokio::test]
async fn service_rejects_legacy_positional_active_resume() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(23);
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    first
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let path = dir.path().join(format!("{thread_id}.json"));
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    stored["continuations"] = serde_json::json!([]);
    stored["served_replies"] = serde_json::json!(["ok"]);
    stored["format_version"] = serde_json::json!(1);
    std::fs::write(&path, serde_json::to_vec_pretty(&stored).expect("encode")).expect("write");
    let second = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = second
        .get_run(thread_id)
        .await
        .expect("legacy")
        .expect("run");
    assert_eq!(loaded.status, WorkflowStatus::Failed);
    assert_eq!(loaded.error.as_deref(), Some("legacy_resume_required"));
    assert!(!loaded.occupies_idle());
}

#[test]
fn empty_agent_reply_completes_when_script_checks_ok() {
    let source = r#"
        let r = agent("Say ok.");
        if r.ok { complete(); } else { ask("wrong reply"); }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(24), source).expect("start");
    assert_eq!(
        run.advance_with_reply(String::new()),
        Ok(WorkflowAdvance::Completed)
    );
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.continuations[0].result, HostCallResult::success(""));
}

#[test]
fn failed_agent_result_is_journaled_and_not_repeated() {
    let source = r#"
        let r = agent("Say ok.");
        pause();
        if !r.ok && r.error == "turn_errored" {
            complete();
        }
    "#;
    let mut run = WorkflowRun::start(ThreadId::from_u128(25), source).expect("start");
    assert_eq!(
        run.advance_with_outcome(HostCallResult::failure("turn_errored")),
        Ok(WorkflowAdvance::Paused)
    );
    assert_eq!(
        run.continuations[0].result,
        HostCallResult::failure("turn_errored")
    );
    run.resume().expect("resume");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.continuations.len(), 2);
}

#[test]
fn replay_divergence_fails_the_run_and_releases_occupancy() {
    let mut run = WorkflowRun::start(
        ThreadId::from_u128(26),
        r#"let r = agent("Say ok."); if r.ok { complete(); }"#,
    )
    .expect("start");
    run.stop().expect("stop");
    run.continuations.clear();
    let mismatch_kind = ContinuationKind::Ask;
    let mismatch_request = serde_json::json!({ "instruction": "other" });
    run.continuations
        .push(codex_workflow_extension::ContinuationRecord {
            seq: 1,
            kind: mismatch_kind,
            request_digest: codex_workflow_extension::request_digest(
                mismatch_kind,
                &mismatch_request,
            ),
            result: HostCallResult::success("x"),
        });
    run.pending_instruction = None;
    run.pending_kind = None;
    run.pending_request_digest = None;
    run.resume().expect("failed closed");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("replay_diverged"));
    assert!(!run.occupies_idle());
}

#[tokio::test]
async fn starting_after_failed_replaces_the_run() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(27);
    service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let failed = service.get_run(thread_id).await.expect("get").expect("run");
    let mut failed = failed;
    failed.fail("host_runtime");
    let path = dir.path().join(format!("{thread_id}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&failed).expect("encode")).expect("write");
    let second = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = second.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(loaded.status, WorkflowStatus::Failed);
    assert!(!loaded.occupies_idle());
    let replaced = second
        .start_run(thread_id, "complete();")
        .await
        .expect("replace");
    assert_eq!(replaced.status, WorkflowStatus::Complete);
    assert_ne!(replaced.run_id, loaded.run_id);
}

#[test]
fn stop_remains_pause_not_failure() {
    let mut run =
        WorkflowRun::start(ThreadId::from_u128(28), yield_then_complete()).expect("start");
    run.stop().expect("stop");
    assert_eq!(run.status, WorkflowStatus::Paused);
    assert_eq!(run.error, None);
}

#[tokio::test]
async fn cancelled_ask_pauses_instead_of_failing() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(29);
    service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let mut run = service.get_run(thread_id).await.expect("get").expect("run");
    run.mark_pending_yield_started();
    std::fs::write(
        dir.path().join(format!("{thread_id}.json")),
        serde_json::to_vec_pretty(&run).expect("encode"),
    )
    .expect("write");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let paused = service
        .finish_yield_turn_with_result(thread_id, HostCallResult::failure("turn_cancelled"))
        .await
        .expect("cancel")
        .expect("run");
    assert_eq!(paused.status, WorkflowStatus::Paused);
    assert_eq!(paused.error, None);
    assert!(!paused.occupies_idle());
}

#[tokio::test]
async fn unreadable_persist_file_is_terminal_failed() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(30);
    std::fs::write(dir.path().join(format!("{thread_id}.json")), b"{not-json").expect("write");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(loaded.status, WorkflowStatus::Failed);
    assert_eq!(loaded.error.as_deref(), Some("unsafe_journal"));
    assert!(!loaded.occupies_idle());
    let replaced = service
        .start_run(thread_id, "complete();")
        .await
        .expect("replace");
    assert_eq!(replaced.status, WorkflowStatus::Complete);
}

#[tokio::test]
async fn service_persists_across_instances() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(4);
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let started = first
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    assert_eq!(started.name, "workflow");
    assert_eq!(started.phase, None);
    assert_eq!(started.status, WorkflowStatus::Active);
    let second = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let loaded = second.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(loaded.run_id, started.run_id);
    assert_eq!(loaded.status, WorkflowStatus::Active);
    let advanced = second.advance_run(thread_id).await.expect("advance");
    assert_eq!(advanced.status, WorkflowStatus::Active);
    assert_eq!(advanced.continuations, started.continuations);
    assert_eq!(advanced.result, serde_json::Value::Null);
}

#[tokio::test]
async fn advance_run_does_not_journal_a_fabricated_host_result() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(34);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let advanced = service.advance_run(thread_id).await.expect("advance");
    assert_eq!(advanced.status, WorkflowStatus::Active);
    assert!(advanced.continuations.is_empty());
    assert_eq!(
        advanced.pending_instruction.as_deref(),
        Some("Compile the crate.")
    );
}

#[tokio::test]
async fn advance_run_cannot_resume_a_pause() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(35);
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    service
        .start_run(thread_id, "pause(); complete();")
        .await
        .expect("start");
    let err = service.advance_run(thread_id).await.expect_err("advance");
    assert!(
        err.to_string().contains("paused"),
        "unexpected error: {err}"
    );
    let paused = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(paused.status, WorkflowStatus::Paused);
    assert!(paused.continuations.is_empty());
}

#[tokio::test]
async fn advance_run_cannot_bypass_a_pending_spawn() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(36);
    let mut started = WorkflowRun::start_with_spawn(
        thread_id,
        r#"
            let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
            if r.ok { complete(); } else { ask("wrong reply"); }
        "#,
        SpawnBinding::Available,
    )
    .expect("start");
    started.mark_pending_yield_started();
    std::fs::write(
        dir.path().join(format!("{thread_id}.json")),
        serde_json::to_vec_pretty(&started).expect("encode"),
    )
    .expect("write");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let advanced = service.advance_run(thread_id).await.expect("advance");
    assert_eq!(advanced.status, WorkflowStatus::Active);
    assert!(advanced.continuations.is_empty());
    assert_eq!(advanced.pending_instruction.as_deref(), Some("Say ok."));
    assert_eq!(advanced.pending_spawn_task_name.as_deref(), Some("review"));
}

#[tokio::test]
async fn starting_while_active_is_rejected() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(5);
    service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("start");
    let err = service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect_err("second start");
    assert!(err.to_string().contains("already active"));
}

#[tokio::test]
async fn starting_after_complete_replaces_the_run() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let thread_id = ThreadId::from_u128(6);
    let completed = service
        .start_run(thread_id, "complete();")
        .await
        .expect("complete");
    assert_eq!(completed.status, WorkflowStatus::Complete);
    let replaced = service
        .start_run(thread_id, yield_then_complete())
        .await
        .expect("restart");
    assert_eq!(replaced.status, WorkflowStatus::Active);
    assert_ne!(replaced.run_id, completed.run_id);
}

#[tokio::test]
async fn named_catalog_start_injects_args_and_phase() {
    let dir = TempDir::new().expect("tempdir");
    let project = TempDir::new().expect("project");
    std::fs::write(
        dir.path().join("demo.rhai"),
        r#"
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
        "#,
    )
    .expect("write catalog script");
    let service = WorkflowService::with_project_root(
        dir.path().to_path_buf(),
        project.path().to_path_buf(),
        std::sync::Weak::new(),
    );
    let mut args = serde_json::Map::new();
    args.insert("topic".into(), serde_json::Value::String("rust".into()));
    let run = service
        .start_named_run(ThreadId::from_u128(12), "demo", args)
        .await
        .expect("start named");
    assert_eq!(run.name, "demo");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(run.phase.as_deref(), Some("Scan"));
}

#[test]
fn spawn_stop_then_resume_replays_journaled_result() {
    let source = r#"
        let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
        pause();
        if r.ok && r.text == "ok" {
            complete();
        }
    "#;
    let mut run =
        WorkflowRun::start_with_spawn(ThreadId::from_u128(18), source, SpawnBinding::Available)
            .expect("start");
    assert_eq!(run.pending_instruction.as_deref(), Some("Say ok."));
    assert_eq!(run.pending_spawn_task_name.as_deref(), Some("review"));
    assert_eq!(
        run.advance_with_reply("ok".to_string()),
        Ok(WorkflowAdvance::Paused)
    );
    assert_eq!(result_replies(&run), vec!["ok".to_string()]);
    assert_eq!(run.pending_spawn_task_name, None);
    run.resume().expect("resume journaled spawn");
    assert_eq!(run.status, WorkflowStatus::Complete);
    assert_eq!(result_replies(&run), vec!["ok".to_string()]);
    assert_eq!(run.pending_instruction, None);
}

#[test]
fn unavailable_spawn_start_is_rejected() {
    let err = WorkflowRun::start(
        ThreadId::from_u128(22),
        r#"agent("Say ok.", #{ "spawn": true, task_name: "review" });"#,
    )
    .expect_err("unavailable");
    assert!(err.contains("unavailable"), "unexpected error: {err}");
}

#[tokio::test]
async fn invalid_rhai_source_is_rejected_by_the_service() {
    let dir = TempDir::new().expect("tempdir");
    let service = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    let err = service
        .start_run(ThreadId::from_u128(7), "???")
        .await
        .expect_err("invalid rhai");
    assert!(
        err.to_string().contains("not valid Rhai"),
        "unexpected error: {err}"
    );
}
