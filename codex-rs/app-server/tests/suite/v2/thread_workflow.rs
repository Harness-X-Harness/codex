use anyhow::Result;
use app_test_support::create_mock_responses_server_sequence_unchecked;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoalGetParams;
use codex_app_server_protocol::ThreadGoalGetResponse;
use codex_app_server_protocol::ThreadGoalSetParams;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadWorkflowAdvanceParams;
use codex_app_server_protocol::ThreadWorkflowAdvanceResponse;
use codex_app_server_protocol::ThreadWorkflowGetParams;
use codex_app_server_protocol::ThreadWorkflowGetResponse;
use codex_app_server_protocol::ThreadWorkflowResumeParams;
use codex_app_server_protocol::ThreadWorkflowResumeResponse;
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::ThreadWorkflowStopParams;
use codex_app_server_protocol::ThreadWorkflowStopResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::sleep;
use tokio::time::timeout;

use super::goal_host_support::AGENT_REQUIRES_OK_RESULT;
use super::goal_host_support::ASK_REQUIRES_OK_REPLY;
use super::goal_host_support::ASK_THEN_COMPLETE;
use super::goal_host_support::COMPLETE_ONLY;
use super::goal_host_support::INVALID_RHAI;
use super::goal_host_support::PARALLEL_REQUIRES_OK_RESULTS;
use super::goal_host_support::READ_TIMEOUT;
use super::goal_host_support::ScriptedHostResponder;
use super::goal_host_support::app_with_features;
use super::goal_host_support::app_with_server;
use super::goal_host_support::create_scripted_host_server;
use super::goal_host_support::goal_host_features;
use super::goal_host_support::request_exposes_tool;
use super::goal_host_support::request_subagent;
use super::goal_host_support::response_requests;
use super::goal_host_support::response_turn_triggers;
use super::goal_host_support::text;
use super::goal_host_support::wait_until_turn_trigger;
use super::goal_host_support::wait_until_workflow_status;

fn start_params(
    thread_id: impl Into<String>,
    source: impl Into<String>,
) -> ThreadWorkflowStartParams {
    ThreadWorkflowStartParams {
        thread_id: thread_id.into(),
        source: source.into(),
        name: None,
        args: None,
    }
}

#[tokio::test]
async fn workflow_rpc_requires_goal_host() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&[Feature::Goals]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/get",
            Some(serde_json::to_value(ThreadWorkflowGetParams {
                thread_id: thread.id,
            })?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("goal_host"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn stock_goals_set_continues_with_update_goal_and_rejects_workflow() -> Result<()> {
    let materialize = responses::sse(vec![
        responses::ev_response_created("resp-materialize"),
        responses::ev_assistant_message("msg-materialize", "Done"),
        responses::ev_completed("resp-materialize"),
    ]);
    let update_goal = responses::sse(vec![
        responses::ev_response_created("resp-update-goal"),
        responses::ev_function_call(
            "call-update-goal",
            "update_goal",
            &json!({ "status": "complete" }).to_string(),
        ),
        responses::ev_completed("resp-update-goal"),
    ]);
    let after_update = responses::sse(vec![
        responses::ev_response_created("resp-after-update"),
        responses::ev_assistant_message("msg-after-update", "Done"),
        responses::ev_completed("resp-after-update"),
    ]);
    let server = create_mock_responses_server_sequence_unchecked(vec![
        materialize,
        update_goal,
        after_update,
    ])
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &[Feature::Goals]).await?;
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
                objective: Some("stock Goals keep worker completion".to_string()),
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

    let requests = response_requests(&server).await?;
    let goal_bodies = requests
        .iter()
        .filter_map(|(trigger, body)| (trigger.as_deref() == Some("goal")).then_some(body))
        .collect::<Vec<_>>();
    assert!(
        !goal_bodies.is_empty(),
        "stock Goals must auto-continue with turn_trigger=goal: {requests:?}"
    );
    assert!(
        goal_bodies
            .iter()
            .any(|body| request_exposes_tool(body, "update_goal")),
        "stock Goals continuation must expose update_goal: {goal_bodies:?}"
    );
    assert!(
        requests
            .iter()
            .all(|(_, body)| request_subagent(body).as_deref() != Some("guardian")),
        "stock Goals must not start a host skeptic panel: {requests:?}"
    );

    let get: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Complete)
    );

    let request_id = app
        .send_raw_request(
            "thread/workflow/get",
            Some(serde_json::to_value(ThreadWorkflowGetParams {
                thread_id: thread.id,
            })?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("goal_host"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn goal_host_set_starts_pursuit_without_update_goal() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread.id.clone(),
        input: vec![text("materialize this thread")],
        ..Default::default()
    })
    .await?;
    let before_goal = response_turn_triggers(&server).await?;
    assert!(
        !before_goal
            .iter()
            .any(|trigger| trigger.as_deref() == Some("goal")),
        "materialize turn should not use the goal trigger: {before_goal:?}"
    );

    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("keep /goal and /workflow distinct".to_string()),
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

    let requests = response_requests(&server).await?;
    let goal_bodies = requests
        .iter()
        .filter_map(|(trigger, body)| (trigger.as_deref() == Some("goal")).then_some(body))
        .collect::<Vec<_>>();
    assert!(
        !goal_bodies.is_empty(),
        "goal_host must start host-owned pursuit with turn_trigger=goal: {requests:?}"
    );
    assert!(
        goal_bodies
            .iter()
            .all(|body| !request_exposes_tool(body, "update_goal")),
        "goal_host pursuit must not expose update_goal: {goal_bodies:?}"
    );

    let get: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(
        get.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );
    Ok(())
}

#[tokio::test]
async fn workflow_start_accepts_rhai_and_rejects_invalid_source() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id.clone(),
                INVALID_RHAI,
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("not valid Rhai"),
        "unexpected error: {}",
        error.error.message
    );

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id, COMPLETE_ONLY),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Complete);
    assert_eq!(started.workflow.result, json!(null));
    Ok(())
}

#[tokio::test]
async fn workflow_complete_persists_this_run_result_without_writing_goal() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;

    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id.clone(),
                r#"complete('x');"#,
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("does not accept"),
        "unexpected error: {}",
        error.error.message
    );

    let none: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(none.workflow, None);

    let empty_thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let empty: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(empty_thread.id, COMPLETE_ONLY),
        })
        .await?;
    assert_eq!(empty.workflow.status, ThreadWorkflowStatus::Complete);
    assert_eq!(empty.workflow.result, json!(null));

    let done_thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let done: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(done_thread.id.clone(), r#"complete("done");"#),
        })
        .await?;
    assert_eq!(done.workflow.status, ThreadWorkflowStatus::Complete);
    assert_eq!(done.workflow.result, json!("done"));

    let get: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: done_thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get.workflow.map(|workflow| workflow.result),
        Some(json!("done"))
    );

    let goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: done_thread.id,
            },
        })
        .await?;
    assert_eq!(goal.goal, None);

    let object_thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let object: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(object_thread.id, r#"complete(#{ ok: true });"#),
        })
        .await?;
    assert_eq!(object.workflow.status, ThreadWorkflowStatus::Complete);
    assert_eq!(object.workflow.result, json!({ "ok": true }));
    Ok(())
}

#[tokio::test]
async fn workflow_rhai_bindings_cannot_commit_goal_state() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id,
                "update_goal();",
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("cannot commit goal"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn workflow_start_continues_with_workflow_trigger_and_does_not_create_a_goal() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    let triggers = wait_until_turn_trigger(&server, "workflow").await?;
    assert!(
        triggers
            .iter()
            .all(|trigger| trigger.as_deref() != Some("goal")),
        "/workflow must not use the goal continuation trigger: {triggers:?}"
    );

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(get_goal.goal, None);

    let completed =
        wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    assert_eq!(
        completed.workflow.map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Complete)
    );

    let get_goal_after: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(get_goal_after.goal, None);
    Ok(())
}

#[tokio::test]
async fn workflow_yield_turn_auto_advances_to_complete() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_auto_advance_injects_assistant_reply() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker: "ok",
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_REQUIRES_OK_REPLY),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_agent_branches_on_structured_host_result() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker: "ok",
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), AGENT_REQUIRES_OK_RESULT),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(get_goal.goal, None);
    Ok(())
}

#[tokio::test]
async fn workflow_parallel_branches_on_ordered_host_results() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker: "ok",
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), PARALLEL_REQUIRES_OK_RESULTS),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(get_goal.goal, None);
    Ok(())
}

#[tokio::test]
async fn workflow_unavailable_spawn_is_rejected_before_a_host_turn() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id,
                r#"agent("Say ok.", #{ "spawn": true, task_name: "review" });"#,
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("unavailable"),
        "unexpected error: {}",
        error.error.message
    );
    let triggers = response_turn_triggers(&server).await?;
    assert!(
        triggers
            .iter()
            .all(|trigger| trigger.as_deref() != Some("workflow")),
        "rejected spawn must not start a host turn: {triggers:?}"
    );
    Ok(())
}

#[tokio::test]
async fn workflow_spawn_without_task_name_is_rejected_before_a_host_turn() -> Result<()> {
    let (mut app, _codex_home, _server) =
        app_with_features(&[Feature::Goals, Feature::GoalHost, Feature::MultiAgentV2]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id,
                r#"agent("Say ok.", #{ "spawn": true, extra: "unused" });"#,
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("task_name"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn workflow_spawn_uses_stock_spawn_agent_and_does_not_write_goal() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker: "ok",
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(
        &server,
        &[Feature::Goals, Feature::GoalHost, Feature::MultiAgentV2],
    )
    .await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(
                thread.id.clone(),
                r#"
                    let r = agent("Say ok.", #{ "spawn": true, task_name: "review" });
                    if r.ok && r.text == "ok" { complete(); } else { ask("wrong reply"); }
                "#,
            ),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;

    let requests = response_requests(&server).await?;
    assert!(
        requests
            .iter()
            .all(|(trigger, _)| trigger.as_deref() != Some("workflow")),
        "spawn must not start a same-Thread workflow turn: {requests:?}"
    );
    assert!(
        requests.iter().any(|(_, body)| {
            let body = body.to_string();
            body.contains("Say ok.") && !body.contains("Active workflow:")
        }),
        "stock spawn_agent must send the prompt to the child: {requests:?}"
    );

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(get_goal.goal, None);
    Ok(())
}

#[tokio::test]
async fn goal_host_set_then_independent_workflow_leaves_goal_active() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
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
                objective: Some("keep /goal and /workflow distinct".to_string()),
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

    let requests = response_requests(&server).await?;
    let goal_bodies = requests
        .iter()
        .filter_map(|(trigger, body)| (trigger.as_deref() == Some("goal")).then_some(body))
        .collect::<Vec<_>>();
    assert!(
        !goal_bodies.is_empty(),
        "setting a goal must start turn_trigger=goal: {requests:?}"
    );
    assert!(
        goal_bodies
            .iter()
            .all(|body| !request_exposes_tool(body, "update_goal")),
        "goal-owned turns must not expose update_goal: {goal_bodies:?}"
    );

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Waiting);

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get_goal.goal.as_ref().map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );

    let triggers = response_turn_triggers(&server).await?;
    assert!(
        triggers
            .iter()
            .all(|trigger| trigger.as_deref() != Some("workflow")),
        "waiting workflow must not start a workflow turn: {triggers:?}"
    );

    let paused: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(paused.goal.status, ThreadGoalStatus::Paused);
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn active_workflow_hold_blocks_goal_idle() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;

    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("workflow hold blocks goal idle".to_string()),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);

    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    loop {
        let workflow: ThreadWorkflowGetResponse = app
            .request(|request_id| ClientRequest::ThreadWorkflowGet {
                request_id,
                params: ThreadWorkflowGetParams {
                    thread_id: thread.id.clone(),
                },
            })
            .await?;
        let workflow_status = workflow.workflow.as_ref().map(|workflow| workflow.status);
        let triggers = response_turn_triggers(&server).await?;
        let goal_count = triggers
            .iter()
            .filter(|trigger| trigger.as_deref() == Some("goal"))
            .count();
        if workflow_status == Some(ThreadWorkflowStatus::Active) {
            assert_eq!(
                goal_count, 0,
                "active workflow must hold goal idle: {triggers:?}"
            );
        } else {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("workflow did not leave Active before hold check timed out");
        }
        sleep(std::time::Duration::from_millis(25)).await;
    }

    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get_goal.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );
    wait_until_turn_trigger(&server, "goal").await?;
    Ok(())
}

#[tokio::test]
async fn paused_workflow_frees_the_slot_for_waiting_goal_how() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker_delay: std::time::Duration::from_millis(400),
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;

    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("paused workflow frees the engine slot".to_string()),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);

    let stopped: ThreadWorkflowStopResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStop {
            request_id,
            params: ThreadWorkflowStopParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(stopped.workflow.status, ThreadWorkflowStatus::Paused);

    let get_goal: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get_goal.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );
    wait_until_turn_trigger(&server, "goal").await?;
    Ok(())
}

#[tokio::test]
async fn waiting_workflow_rejects_a_second_start() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
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
                objective: Some("one waiting workflow at a time".to_string()),
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

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Waiting);

    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(start_params(
                thread.id,
                COMPLETE_ONLY,
            ))?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("already active"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn workflow_stop_pauses_and_resume_returns_to_active() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker_delay: std::time::Duration::from_millis(400),
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    let stopped: ThreadWorkflowStopResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStop {
            request_id,
            params: ThreadWorkflowStopParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(stopped.workflow.status, ThreadWorkflowStatus::Paused);

    let paused: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        paused.workflow.map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Paused)
    );

    let resumed: ThreadWorkflowResumeResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowResume {
            request_id,
            params: ThreadWorkflowResumeParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(resumed.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_advance_is_optional_override() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker_delay: std::time::Duration::from_millis(400),
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(thread.id.clone(), ASK_THEN_COMPLETE),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    let advanced: ThreadWorkflowAdvanceResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowAdvance {
            request_id,
            params: ThreadWorkflowAdvanceParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(advanced.workflow.status, ThreadWorkflowStatus::Complete);

    let get: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(
        get.workflow.map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Complete)
    );
    Ok(())
}

#[tokio::test]
async fn workflow_start_by_name_loads_user_library() -> Result<()> {
    let (mut app, codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let library = codex_home.path().join("workflows");
    std::fs::create_dir_all(&library)?;
    std::fs::write(
        library.join("demo.rhai"),
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
    )?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let mut args = std::collections::HashMap::new();
    args.insert(
        "topic".to_string(),
        serde_json::Value::String("rust".to_string()),
    );
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id,
                source: String::new(),
                name: Some("demo".to_string()),
                args: Some(args),
            },
        })
        .await?;
    assert_eq!(started.workflow.name, "demo");
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Complete);
    Ok(())
}

#[tokio::test]
async fn named_workflow_waits_when_goal_occupies() -> Result<()> {
    let (mut app, codex_home, server) = app_with_features(&goal_host_features()).await?;
    let library = codex_home.path().join("workflows");
    std::fs::create_dir_all(&library)?;
    std::fs::write(
        library.join("demo.rhai"),
        r#"
            let meta = #{
                name: "demo",
                description: "named",
            };
            complete();
        "#,
    )?;
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
                objective: Some("named catalog waits for occupancy".to_string()),
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

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: String::new(),
                name: Some("demo".to_string()),
                args: None,
            },
        })
        .await?;
    assert_eq!(started.workflow.name, "demo");
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Waiting);

    let triggers = response_turn_triggers(&server).await?;
    assert!(
        triggers
            .iter()
            .all(|trigger| trigger.as_deref() != Some("workflow")),
        "waiting named workflow must not start a workflow turn: {triggers:?}"
    );

    let paused: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(paused.goal.status, ThreadGoalStatus::Paused);
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_start_by_name_rejects_filename_mismatch() -> Result<()> {
    let (mut app, codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let library = codex_home.path().join("workflows");
    std::fs::create_dir_all(&library)?;
    std::fs::write(
        library.join("demo.rhai"),
        r#"let meta = #{ name: "other", description: "mismatch" }; complete();"#,
    )?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(ThreadWorkflowStartParams {
                thread_id: thread.id,
                source: String::new(),
                name: Some("demo".to_string()),
                args: None,
            })?),
        )
        .await?;
    let error: JSONRPCError = timeout(
        READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert!(
        error.error.message.contains("must match meta.name"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}

#[tokio::test]
async fn workflow_empty_agent_text_is_successful() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker: "",
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(
                thread.id.clone(),
                r#"
                    let r = agent("Say ok.");
                    if r.ok { complete(); } else { ask("wrong reply"); }
                "#,
            ),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_runtime_failure_exposes_failed_and_frees_the_slot() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: start_params(
                thread.id.clone(),
                r#"ask("Compile the crate."); unknown_fn();"#,
            ),
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    let failed =
        wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Failed).await?;
    let workflow = failed.workflow.expect("failed workflow");
    assert_eq!(workflow.status, ThreadWorkflowStatus::Failed);
    assert_eq!(workflow.error.as_deref(), Some("host_runtime"));
    assert!(
        workflow
            .error
            .as_deref()
            .is_some_and(|error| !error.contains("provider") && !error.contains("http")),
        "failure error must stay secret-safe: {:?}",
        workflow.error
    );

    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("failed workflow frees the engine slot".to_string()),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);
    wait_until_turn_trigger(&server, "goal").await?;

    let get: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(
        get.workflow.as_ref().map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Failed)
    );
    Ok(())
}
