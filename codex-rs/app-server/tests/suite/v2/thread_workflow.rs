use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
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
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use pretty_assertions::assert_eq;
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

async fn app_with_features(features: &[Feature]) -> Result<(TestAppServer, TempDir, MockServer)> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
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
    Ok((app, codex_home, server))
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
            let metadata: serde_json::Value = serde_json::from_str(header.to_str()?)?;
            Ok(metadata
                .get("turn_trigger")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string))
        })
        .collect()
}

fn text(value: &str) -> UserInput {
    UserInput::Text {
        text: value.to_string(),
        text_elements: Vec::new(),
    }
}
