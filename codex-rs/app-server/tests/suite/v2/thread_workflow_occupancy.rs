use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoalGetParams;
use codex_app_server_protocol::ThreadGoalGetResponse;
use codex_app_server_protocol::ThreadGoalSetParams;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadWorkflowGetParams;
use codex_app_server_protocol::ThreadWorkflowGetResponse;
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use pretty_assertions::assert_eq;
use tokio::time::sleep;
use tokio::time::timeout;

use super::goal_host_support::ASK_THEN_COMPLETE;
use super::goal_host_support::INVALID_RHAI;
use super::goal_host_support::READ_TIMEOUT;
use super::goal_host_support::ScriptedHostResponder;
use super::goal_host_support::app_with_server;
use super::goal_host_support::create_scripted_host_server;
use super::goal_host_support::goal_host_features;
use super::goal_host_support::response_turn_triggers;
use super::goal_host_support::wait_until_turn_trigger;

#[tokio::test]
async fn rejected_workflow_start_leaves_the_slot_for_goal_how() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker_delay: std::time::Duration::from_millis(400),
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, _codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;

    let request_id = app
        .send_raw_request(
            "thread/workflow/start",
            Some(serde_json::to_value(ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: INVALID_RHAI.to_string(),
                name: None,
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
        error.error.message.contains("not valid Rhai"),
        "unexpected error: {}",
        error.error.message
    );

    let set: ThreadGoalSetResponse = app
        .request(|request_id| ClientRequest::ThreadGoalSet {
            request_id,
            params: ThreadGoalSetParams {
                thread_id: thread.id,
                objective: Some(
                    "rejected start must not leave a phantom workflow claim".to_string(),
                ),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);
    wait_until_turn_trigger(&server, "goal").await?;
    Ok(())
}

#[tokio::test]
async fn restore_of_active_workflow_and_goal_keeps_one_owner() -> Result<()> {
    let server = create_scripted_host_server(ScriptedHostResponder {
        worker_delay: std::time::Duration::from_millis(400),
        ..ScriptedHostResponder::default()
    })
    .await;
    let (mut app, codex_home) = app_with_server(&server, &goal_host_features()).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ASK_THEN_COMPLETE.to_string(),
                name: None,
                args: None,
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
                objective: Some("restore must keep one engine occupant".to_string()),
                status: None,
                token_budget: None,
            },
        })
        .await?;
    assert_eq!(set.goal.status, ThreadGoalStatus::Active);

    let triggers_before = response_turn_triggers(&server).await?;
    let goal_before = triggers_before
        .iter()
        .filter(|trigger| trigger.as_deref() == Some("goal"))
        .count();
    assert_eq!(
        goal_before, 0,
        "goal HOW must wait while the workflow is active: {triggers_before:?}"
    );

    drop(app);
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_managed_config()
        .build_initialized()
        .await?;
    let resume_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse =
        timeout(READ_TIMEOUT, app.read_response(resume_id)).await??;
    assert_eq!(resumed.thread.id, thread.id);

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
        let goal: ThreadGoalGetResponse = app
            .request(|request_id| ClientRequest::ThreadGoalGet {
                request_id,
                params: ThreadGoalGetParams {
                    thread_id: thread.id.clone(),
                },
            })
            .await?;
        assert_eq!(
            goal.goal.as_ref().map(|goal| goal.status),
            Some(ThreadGoalStatus::Active)
        );
        let workflow_status = workflow.workflow.as_ref().map(|workflow| workflow.status);
        let triggers = response_turn_triggers(&server).await?;
        let goal_after = triggers
            .iter()
            .filter(|trigger| trigger.as_deref() == Some("goal"))
            .count();
        let new_goal = goal_after.saturating_sub(goal_before);
        if workflow_status == Some(ThreadWorkflowStatus::Active) {
            assert_eq!(
                new_goal, 0,
                "restored active workflow must not share the slot: {triggers:?}"
            );
        }
        if workflow_status == Some(ThreadWorkflowStatus::Active)
            || workflow_status == Some(ThreadWorkflowStatus::Waiting)
            || workflow_status == Some(ThreadWorkflowStatus::Complete)
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "restore did not publish a workflow occupant: status={workflow_status:?}"
            );
        }
        sleep(std::time::Duration::from_millis(25)).await;
    }
    Ok(())
}
