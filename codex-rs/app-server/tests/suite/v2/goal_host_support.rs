use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadGoalGetParams;
use codex_app_server_protocol::ThreadGoalGetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadWorkflowGetParams;
use codex_app_server_protocol::ThreadWorkflowGetResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use serde_json::Value;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

pub(super) const READ_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);
pub(super) const ASK_THEN_COMPLETE: &str = r#"ask("Compile the crate."); complete();"#;
pub(super) const COMPLETE_ONLY: &str = "complete();";
pub(super) const MARKDOWN_STEP_TABLE: &str = "# Ship\n\n## Build\nCompile the crate.\n";
pub(super) const CONTINUE_VERDICT: &str = r#"{"decision":"continue","evidence":"objective still open","next_step":"keep working","blocker_key":""}"#;
pub(super) const CANDIDATE_COMPLETE_VERDICT: &str = r#"{"decision":"candidate_complete","evidence":"deliverable exists","next_step":"stop","blocker_key":""}"#;
pub(super) const BLOCKED_VERDICT: &str = r#"{"decision":"blocked","evidence":"same blocker persists","next_step":"retry the blocked work","blocker_key":"missing_tests"}"#;
pub(super) const INVALID_EVALUATOR: &str = "not-json";
pub(super) const SKEPTIC_PASS: &str =
    r#"{"refuted":false,"evidence":"objective is corroborated","next_step":"none"}"#;
pub(super) const SKEPTIC_REFUTE: &str =
    r#"{"refuted":true,"evidence":"missing proof","next_step":"add tests"}"#;
pub(super) const ASK_REQUIRES_OK_REPLY: &str =
    r#"let x = ask("Say ok."); if x == "ok" { complete(); } else { ask("wrong reply"); }"#;

pub(super) async fn app_with_features(
    features: &[Feature],
) -> Result<(TestAppServer, TempDir, MockServer)> {
    let server = if features.contains(&Feature::GoalHost) {
        create_scripted_host_server(ScriptedHostResponder::default()).await
    } else {
        create_repeating_assistant_server("Done").await
    };
    let (app, codex_home) = app_with_server(&server, features).await?;
    Ok((app, codex_home, server))
}

pub(super) async fn app_with_server(
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

pub(super) async fn create_repeating_assistant_server(message: &str) -> MockServer {
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_assistant_message("msg-1", message),
        responses::ev_completed("resp-1"),
    ]);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(responses::sse_response(body))
        .mount(&server)
        .await;
    server
}

pub(super) async fn create_scripted_host_server(responder: ScriptedHostResponder) -> MockServer {
    let server = responses::start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(responder)
        .mount(&server)
        .await;
    server
}

#[derive(Clone, Copy)]
pub(super) struct ScriptedHostResponder {
    pub worker: &'static str,
    pub evaluator: &'static str,
    pub skeptic: &'static str,
}

impl Default for ScriptedHostResponder {
    fn default() -> Self {
        Self {
            worker: "Done",
            evaluator: CONTINUE_VERDICT,
            skeptic: SKEPTIC_PASS,
        }
    }
}

impl Respond for ScriptedHostResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body = String::from_utf8_lossy(&request.body);
        let message = if body.contains("\"refuted\"") {
            self.skeptic
        } else if body.contains("candidate_complete") {
            self.evaluator
        } else {
            self.worker
        };
        responses::sse_response(responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", message),
            responses::ev_completed("resp-1"),
        ]))
    }
}

pub(super) async fn response_turn_triggers(server: &MockServer) -> Result<Vec<Option<String>>> {
    Ok(response_requests(server)
        .await?
        .into_iter()
        .map(|(trigger, _)| trigger)
        .collect())
}

pub(super) async fn wait_until_turn_trigger(
    server: &MockServer,
    expected: &str,
) -> Result<Vec<Option<String>>> {
    wait_until_turn_trigger_count(server, expected, /*count*/ 1).await
}

pub(super) async fn wait_until_turn_trigger_count(
    server: &MockServer,
    expected: &str,
    count: usize,
) -> Result<Vec<Option<String>>> {
    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    loop {
        let triggers = response_turn_triggers(server).await?;
        let observed = triggers
            .iter()
            .filter(|trigger| trigger.as_deref() == Some(expected))
            .count();
        if observed >= count {
            return Ok(triggers);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("{expected} trigger count {count} not observed: {triggers:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) async fn wait_until_workflow_status(
    app: &mut TestAppServer,
    thread_id: &str,
    expected: ThreadWorkflowStatus,
) -> Result<ThreadWorkflowGetResponse> {
    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    loop {
        let get: ThreadWorkflowGetResponse = app
            .request(|request_id| ClientRequest::ThreadWorkflowGet {
                request_id,
                params: ThreadWorkflowGetParams {
                    thread_id: thread_id.to_string(),
                },
            })
            .await?;
        if get
            .workflow
            .as_ref()
            .is_some_and(|workflow| workflow.status == expected)
        {
            return Ok(get);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("workflow status {expected:?} not observed: {get:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) async fn wait_until_goal_status(
    app: &mut TestAppServer,
    thread_id: &str,
    expected: ThreadGoalStatus,
) -> Result<ThreadGoalGetResponse> {
    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    loop {
        let get: ThreadGoalGetResponse = app
            .request(|request_id| ClientRequest::ThreadGoalGet {
                request_id,
                params: ThreadGoalGetParams {
                    thread_id: thread_id.to_string(),
                },
            })
            .await?;
        if get
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status == expected)
        {
            return Ok(get);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("goal status {expected:?} not observed: {get:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) async fn response_requests(server: &MockServer) -> Result<Vec<(Option<String>, Value)>> {
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

pub(super) fn request_subagent(body: &Value) -> Option<String> {
    body.pointer("/client_metadata/x-openai-subagent")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn request_exposes_tool(body: &Value, tool_name: &str) -> bool {
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

pub(super) fn text(value: &str) -> UserInput {
    UserInput::Text {
        text: value.to_string(),
        text_elements: Vec::new(),
    }
}

pub(super) fn goal_host_features() -> [Feature; 2] {
    [Feature::Goals, Feature::GoalHost]
}
