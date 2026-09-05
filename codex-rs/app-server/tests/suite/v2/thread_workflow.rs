use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
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
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::MockServer;

const READ_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);
const ONE_STEP_WORKFLOW: &str = "# Ship\n\n## Build\nCompile the crate.\n";

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
async fn goal_host_set_does_not_start_goal_continuation() -> Result<()> {
    let (mut app, _codex_home, server) =
        app_with_features(&[Feature::Goals, Feature::GoalHost]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread.id.clone(),
        input: vec![text("materialize this thread")],
        ..Default::default()
    })
    .await?;
    let before_goal = response_turn_triggers(&server).await?;
    assert_eq!(before_goal.len(), 1);
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
    let after_goal = response_turn_triggers(&server).await?;
    assert_eq!(after_goal, before_goal);
    assert!(
        !after_goal
            .iter()
            .any(|trigger| trigger.as_deref() == Some("goal")),
        "goal_host GoalHow::Workflow must not auto-continue with turn_trigger=goal: {after_goal:?}"
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
async fn workflow_start_continues_with_workflow_trigger_and_does_not_create_a_goal() -> Result<()> {
    let (mut app, _codex_home, server) =
        app_with_features(&[Feature::Goals, Feature::GoalHost]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ONE_STEP_WORKFLOW.to_string(),
            },
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    timeout(
        READ_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let triggers = response_turn_triggers(&server).await?;
    assert!(
        triggers
            .iter()
            .any(|trigger| trigger.as_deref() == Some("workflow")),
        "workflow start should continue with turn_trigger=workflow: {triggers:?}"
    );
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

    let advanced: ThreadWorkflowAdvanceResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowAdvance {
            request_id,
            params: ThreadWorkflowAdvanceParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(advanced.workflow.status, ThreadWorkflowStatus::Complete);

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
async fn goal_host_set_then_independent_workflow_leaves_goal_active() -> Result<()> {
    let (mut app, _codex_home, server) =
        app_with_features(&[Feature::Goals, Feature::GoalHost]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread.id.clone(),
        input: vec![text("materialize this thread")],
        ..Default::default()
    })
    .await?;
    let before_goal = response_turn_triggers(&server).await?;

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
    let after_goal = response_turn_triggers(&server).await?;
    assert_eq!(after_goal, before_goal);
    assert!(
        !after_goal
            .iter()
            .any(|trigger| trigger.as_deref() == Some("goal")),
        "setting a goal must not start turn_trigger=goal: {after_goal:?}"
    );

    let started: ThreadWorkflowStartResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowStart {
            request_id,
            params: ThreadWorkflowStartParams {
                thread_id: thread.id.clone(),
                source: ONE_STEP_WORKFLOW.to_string(),
            },
        })
        .await?;
    assert_eq!(started.workflow.status, ThreadWorkflowStatus::Active);

    timeout(
        READ_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let triggers = response_turn_triggers(&server).await?;
    assert!(
        triggers
            .iter()
            .any(|trigger| trigger.as_deref() == Some("workflow")),
        "workflow start should continue with turn_trigger=workflow: {triggers:?}"
    );
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

    let advanced: ThreadWorkflowAdvanceResponse = app
        .request(|request_id| ClientRequest::ThreadWorkflowAdvance {
            request_id,
            params: ThreadWorkflowAdvanceParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    assert_eq!(advanced.workflow.status, ThreadWorkflowStatus::Complete);

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

async fn app_with_features(features: &[Feature]) -> Result<(TestAppServer, TempDir, MockServer)> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let (app, codex_home) = app_with_server(&server, features).await?;
    Ok((app, codex_home, server))
}

async fn app_with_server(
    server: &MockServer,
    features: &[Feature],
) -> Result<(TestAppServer, TempDir)> {
    let codex_home = TempDir::new()?;
    let mut config = MockResponsesConfig::new(&server.uri());
    for feature in features {
        config = config.enable_feature(*feature);
    }
    config.write(codex_home.path())?;
    let app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_managed_config()
        .build_initialized()
        .await?;
    Ok((app, codex_home))
}

async fn response_turn_triggers(server: &MockServer) -> Result<Vec<Option<String>>> {
    let requests = server
        .received_requests()
        .await
        .context("wiremock should record response requests")?;
    requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(|request| {
            let Some(header) = request.headers.get("x-codex-turn-metadata") else {
                return Ok(None);
            };
            let metadata: Value = serde_json::from_str(header.to_str()?)?;
            Ok(metadata
                .get("turn_trigger")
                .and_then(Value::as_str)
                .map(str::to_string))
        })
        .collect()
}

async fn response_requests(server: &MockServer) -> Result<Vec<(Option<String>, Value)>> {
    let requests = server
        .received_requests()
        .await
        .context("wiremock should record response requests")?;
    requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(|request| {
            let trigger = request
                .headers
                .get("x-codex-turn-metadata")
                .and_then(|header| header.to_str().ok())
                .and_then(|header| serde_json::from_str::<Value>(header).ok())
                .and_then(|metadata| {
                    metadata
                        .get("turn_trigger")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            let body: Value = serde_json::from_slice(&request.body)?;
            Ok((trigger, body))
        })
        .collect()
}

fn request_subagent(body: &Value) -> Option<String> {
    body.pointer("/client_metadata/x-openai-subagent")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn request_exposes_tool(body: &Value, tool_name: &str) -> bool {
    if let Some(tools) = body.get("tools").and_then(Value::as_array)
        && tools_include(tools, tool_name)
    {
        return true;
    }
    let Some(input) = body.get("input").and_then(Value::as_array) else {
        return false;
    };
    input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("additional_tools")
            && item
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| tools_include(tools, tool_name))
    })
}

fn tools_include(tools: &[Value], tool_name: &str) -> bool {
    tools.iter().any(|tool| {
        tool.get("name").and_then(Value::as_str) == Some(tool_name)
            || tool
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|children| tools_include(children, tool_name))
    })
}

fn text(value: &str) -> UserInput {
    UserInput::Text {
        text: value.to_string(),
        text_elements: Vec::new(),
    }
}
