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
use codex_app_server_protocol::ThreadWorkflowGetParams;
use codex_app_server_protocol::ThreadWorkflowGetResponse;
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::TurnStartParams;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::sleep;
use tokio::time::timeout;

use super::goal_host_support::ASK_REQUIRES_OK_REPLY;
use super::goal_host_support::ASK_THEN_COMPLETE;
use super::goal_host_support::COMPLETE_ONLY;
use super::goal_host_support::MARKDOWN_STEP_TABLE;
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
async fn workflow_start_accepts_rhai_and_rejects_markdown() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: MARKDOWN_STEP_TABLE.to_string(),
            })?),
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
            params: ThreadWorkflowStartParams {
                thread_id: thread.id,
                source: COMPLETE_ONLY.to_string(),
            },
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Complete);
    Ok(())
}

#[tokio::test]
async fn workflow_rhai_bindings_cannot_commit_goal_state() -> Result<()> {
    let (mut app, _codex_home, _server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(ThreadWorkflowStartParams {
                thread_id: thread.id,
                source: "update_goal();".to_string(),
            })?),
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
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_THEN_COMPLETE.to_string(),
            },
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
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_THEN_COMPLETE.to_string(),
            },
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
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_REQUIRES_OK_REPLY.to_string(),
            },
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);
    wait_until_turn_trigger(&server, "workflow").await?;
    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;
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
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_THEN_COMPLETE.to_string(),
            },
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    let _triggers = wait_until_turn_trigger(&server, "workflow").await?;

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

    let get_workflow: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get_workflow
            .workflow
            .as_ref()
            .map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Active)
    );

    wait_until_workflow_status(&mut app, &thread.id, ThreadWorkflowStatus::Complete).await?;

    let get_goal_after: ThreadGoalGetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalGet {
            request_id,
            params: ThreadGoalGetParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(
        get_goal_after.goal.map(|goal| goal.status),
        Some(ThreadGoalStatus::Active)
    );

    let get_workflow_after: ThreadWorkflowGetResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowGet {
            request_id,
            params: ThreadWorkflowGetParams {
                thread_id: thread.id,
            },
        })
        .await?;
    assert_eq!(
        get_workflow_after.workflow.map(|workflow| workflow.status),
        Some(ThreadWorkflowStatus::Complete)
    );
    Ok(())
}

#[tokio::test]
async fn active_workflow_hold_blocks_goal_idle() -> Result<()> {
    let (mut app, _codex_home, server) = app_with_features(&goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_THEN_COMPLETE.to_string(),
            },
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
    Ok(())
}
