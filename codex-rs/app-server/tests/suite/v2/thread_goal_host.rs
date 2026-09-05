use anyhow::Result;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadGoalSetParams;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use pretty_assertions::assert_eq;
use tokio::time::timeout;

use super::goal_host_support::BLOCKED_VERDICT;
use super::goal_host_support::CANDIDATE_COMPLETE_VERDICT;
use super::goal_host_support::INVALID_EVALUATOR;
use super::goal_host_support::READ_TIMEOUT;
use super::goal_host_support::SKEPTIC_REFUTE;
use super::goal_host_support::ScriptedHostResponder;
use super::goal_host_support::app_with_features;
use super::goal_host_support::app_with_server;
use super::goal_host_support::create_scripted_host_server;
use super::goal_host_support::goal_host_features;
use super::goal_host_support::text;
use super::goal_host_support::wait_until_goal_status;
use super::goal_host_support::wait_until_turn_trigger_count;

async fn materialize_and_set_goal(
    app: &mut app_test_support::TestAppServer,
    objective: &str,
) -> Result<String> {
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread.id.clone(),
        input: vec![text("materialize this thread")],
        ..Default::default()
    })
    .await?;
    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some(objective.to_string()),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);
    timeout(
        READ_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    Ok(thread.id)
}

#[tokio::test]
async fn host_evaluate_continue_starts_second_goal_turn() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread_id = materialize_and_set_goal(&mut app, "keep pursuing").await?;
    wait_until_turn_trigger_count(&server, "goal", /*count*/ 2).await?;
    let get = wait_until_goal_status(&mut app, &thread_id, ThreadGoalStatus::Active).await?;
    assert_eq!(
        get.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );
    Ok(())
}

#[tokio::test]
async fn host_evaluate_invalid_json_pauses_goal() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        evaluator: INVALID_EVALUATOR,
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread_id = materialize_and_set_goal(&mut app, "pause on evaluator failure").await?;
    wait_until_goal_status(&mut app, &thread_id, ThreadGoalStatus::Paused).await?;
    Ok(())
}

#[tokio::test]
async fn host_evaluate_blocked_streak_marks_blocked() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        evaluator: BLOCKED_VERDICT,
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread_id =
        materialize_and_set_goal(&mut app, "blocked after three identical keys").await?;
    wait_until_goal_status(&mut app, &thread_id, ThreadGoalStatus::Blocked).await?;
    Ok(())
}

#[tokio::test]
async fn host_evaluate_candidate_complete_and_skeptics_mark_complete() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        evaluator: CANDIDATE_COMPLETE_VERDICT,
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread_id = materialize_and_set_goal(&mut app, "complete after skeptics confirm").await?;
    wait_until_goal_status(&mut app, &thread_id, ThreadGoalStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn host_skeptics_refute_pauses_goal() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        evaluator: CANDIDATE_COMPLETE_VERDICT,
        skeptic: SKEPTIC_REFUTE,
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread_id = materialize_and_set_goal(&mut app, "pause when skeptics refute").await?;
    wait_until_goal_status(&mut app, &thread_id, ThreadGoalStatus::Paused).await?;
    Ok(())
}
