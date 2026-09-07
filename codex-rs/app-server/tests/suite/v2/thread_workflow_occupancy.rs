use anyhow::Result;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoalSetParams;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadWorkflowStartParams;
use pretty_assertions::assert_eq;
use tokio::time::timeout;

use super::goal_host_support::INVALID_RHAI;
use super::goal_host_support::READ_TIMEOUT;
use super::goal_host_support::ScriptedHostResponder;
use super::goal_host_support::app_with_server;
use super::goal_host_support::create_scripted_host_server;
use super::goal_host_support::goal_host_features;
use super::goal_host_support::wait_until_turn_trigger;

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
