//! Deterministic Grok Provider-binding lifecycle gates for stock Codex 0.154.
//!
//! The product invariant is intentionally expressed through App Server APIs:
//! a thread started with the Grok profile remains bound to that provider and
//! model across ordinary continuation, fork, cold resume, and compaction.
//! These tests do not introduce lifecycle ownership for Grok; they prove that
//! stock 0.154 durable thread settings remain authoritative.
//!
//! Durable identity is the stock `model_provider` id on the thread/session.
//! Resume, fork, and compaction reconstruct the runtime provider from the
//! current App Server process profile map. `wire_api = "grok_responses"` is
//! the serialized Grok selector; HTTP transport stays stock Responses.

use super::compaction::wait_for_context_compaction_completed;
use super::compaction::wait_for_context_compaction_started;
use super::compaction::wait_for_turn_completed;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput as V2UserInput;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::ResponseTemplate;

// macOS and Windows Bazel CI can spend tens of seconds starting app-server
// subprocesses or processing test RPCs under load.
#[cfg(any(target_os = "macos", windows))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

const GROK_PROVIDER: &str = "grok";
const GROK_MODEL: &str = "grok-4.6";
const SEED_PROMPT: &str = "seed history";
const SEED_REPLY: &str = "SEED_REPLY";

fn grok_profile(server_uri: &str) -> MockResponsesConfig {
    MockResponsesConfig::new(server_uri)
        .with_model(GROK_MODEL)
        .with_model_provider(GROK_PROVIDER)
        .with_provider_name("Grok")
        .with_provider_base_url(&format!("{server_uri}/api/codex"))
        .with_wire_api("grok_responses")
        .with_provider_config("supports_websockets = false\nrequires_openai_auth = false")
}

fn reply(id: &str, text: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse(vec![
        responses::ev_assistant_message(id, text),
        responses::ev_completed_with_tokens(id, /*total_tokens*/ 120),
    ]))
}

async fn build_app(codex_home: &std::path::Path) -> Result<TestAppServer> {
    TestAppServer::builder()
        .with_codex_home(codex_home)
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await
}

async fn start_grok_thread(mcp: &mut TestAppServer) -> Result<String> {
    let request = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let ThreadStartResponse {
        thread,
        model,
        model_provider,
        ..
    } = timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(
        (model_provider.as_str(), model.as_str()),
        (GROK_PROVIDER, GROK_MODEL)
    );
    assert_eq!(thread.model_provider, GROK_PROVIDER);
    Ok(thread.id)
}

async fn send_turn(mcp: &mut TestAppServer, thread_id: &str, text: &str) -> Result<TurnStatus> {
    let request = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![V2UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let TurnStartResponse { turn } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    loop {
        let completed: TurnCompletedNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_notification("turn/completed"),
        )
        .await??;
        if completed.turn.id == turn.id {
            return Ok(completed.turn.status);
        }
    }
}

fn input_position(input: &[serde_json::Value], needle: &str) -> Option<usize> {
    input
        .iter()
        .position(|item| item.to_string().contains(needle))
}

fn assert_all_requests_are_grok(mock: &responses::ResponseMock, expected: usize) {
    let requests = mock.requests();
    assert_eq!(requests.len(), expected);
    for request in &requests {
        assert_eq!(request.path(), "/api/codex/responses");
        assert_eq!(request.body_json()["model"], GROK_MODEL);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_normal_continuation_keeps_provider_binding() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![
            reply("seed", SEED_REPLY),
            reply("continued", "CONTINUED_REPLY"),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    grok_profile(&server.uri()).write(codex_home.path())?;
    let mut mcp = build_app(codex_home.path()).await?;

    let thread_id = start_grok_thread(&mut mcp).await?;
    assert_eq!(
        send_turn(&mut mcp, &thread_id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    assert_eq!(
        send_turn(&mut mcp, &thread_id, "ordinary continuation").await?,
        TurnStatus::Completed
    );

    let request = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.clone(),
            include_turns: false,
        })
        .await?;
    let ThreadReadResponse { thread } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(
        (thread.model_provider.as_str(), thread.model.as_deref()),
        (GROK_PROVIDER, Some(GROK_MODEL))
    );

    assert_all_requests_are_grok(&mock, 2);
    let continued_input = mock.requests()[1].input();
    let seed_prompt =
        input_position(&continued_input, SEED_PROMPT).expect("continuation replays seed");
    let seed_reply =
        input_position(&continued_input, SEED_REPLY).expect("continuation replays reply");
    let follow_up = input_position(&continued_input, "ordinary continuation")
        .expect("continuation sends follow-up");
    assert!(seed_prompt < seed_reply && seed_reply < follow_up);

    timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_fork_keeps_provider_binding_and_isolates_branch_failure() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![
            reply("seed", SEED_REPLY),
            reply("fork-1", "FORK_REPLY"),
            reply("source-1", "SOURCE_REPLY"),
            ResponseTemplate::new(500),
            reply("source-2", "SOURCE_REPLY_AFTER_FORK_FAILURE"),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    grok_profile(&server.uri()).write(codex_home.path())?;
    let mut mcp = build_app(codex_home.path()).await?;

    let source_id = start_grok_thread(&mut mcp).await?;
    assert_eq!(
        send_turn(&mut mcp, &source_id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );

    let request = mcp
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source_id.clone(),
            ..Default::default()
        })
        .await?;
    let ThreadForkResponse {
        thread: fork,
        model,
        model_provider,
        ..
    } = timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_ne!(fork.id, source_id);
    assert_eq!(
        (model_provider.as_str(), model.as_str()),
        (GROK_PROVIDER, GROK_MODEL)
    );
    assert_eq!(fork.model_provider, GROK_PROVIDER);

    assert_eq!(
        send_turn(&mut mcp, &fork.id, "fork follow-up").await?,
        TurnStatus::Completed
    );
    assert_eq!(
        send_turn(&mut mcp, &source_id, "source follow-up").await?,
        TurnStatus::Completed
    );
    assert_eq!(
        send_turn(&mut mcp, &fork.id, "fork follow-up that fails").await?,
        TurnStatus::Failed
    );
    assert_eq!(
        send_turn(&mut mcp, &source_id, "source continues").await?,
        TurnStatus::Completed
    );

    assert_all_requests_are_grok(&mock, 5);
    let requests = mock.requests();
    let fork_input = requests[1].input();
    let seed_prompt = input_position(&fork_input, SEED_PROMPT).expect("fork replays seed prompt");
    let seed_reply = input_position(&fork_input, SEED_REPLY).expect("fork replays seed reply");
    let fork_prompt =
        input_position(&fork_input, "fork follow-up").expect("fork sends its own prompt");
    assert!(seed_prompt < seed_reply && seed_reply < fork_prompt);
    let source_after_failure = requests[4].input();
    assert!(input_position(&source_after_failure, "fork follow-up").is_none());
    assert!(input_position(&source_after_failure, "source continues").is_some());

    timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_cold_restart_resume_keeps_provider_binding() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![reply("seed", SEED_REPLY), reply("resumed", "RESUMED_REPLY")],
    )
    .await;
    let codex_home = TempDir::new()?;
    grok_profile(&server.uri()).write(codex_home.path())?;
    let grok_config = std::fs::read_to_string(codex_home.path().join("config.toml"))?;

    let thread_id = {
        let mut mcp = build_app(codex_home.path()).await?;
        let thread_id = start_grok_thread(&mut mcp).await?;
        assert_eq!(
            send_turn(&mut mcp, &thread_id, SEED_PROMPT).await?,
            TurnStatus::Completed
        );
        timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
        thread_id
    };

    {
        MockResponsesConfig::new(&server.uri()).write(codex_home.path())?;
        let mut mcp = build_app(codex_home.path()).await?;
        let request = mcp
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            })
            .await?;
        let error: JSONRPCError = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        assert!(
            error.error.message.contains(GROK_PROVIDER),
            "resume should name the missing Grok profile: {}",
            error.error.message
        );
        assert_eq!(mock.requests().len(), 1);
        timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
    }

    std::fs::write(codex_home.path().join("config.toml"), grok_config)?;
    let mut mcp = build_app(codex_home.path()).await?;
    let request = mcp
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let ThreadResumeResponse {
        thread,
        model,
        model_provider,
        ..
    } = timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(thread.id, thread_id);
    assert_eq!(
        (model_provider.as_str(), model.as_str()),
        (GROK_PROVIDER, GROK_MODEL)
    );
    assert_eq!(thread.model_provider, GROK_PROVIDER);
    assert_eq!(
        send_turn(&mut mcp, &thread_id, "resumed follow-up").await?,
        TurnStatus::Completed
    );

    assert_all_requests_are_grok(&mock, 2);
    let resumed_input = mock.requests()[1].input();
    let seed_prompt = input_position(&resumed_input, SEED_PROMPT).expect("resume replays seed");
    let seed_reply = input_position(&resumed_input, SEED_REPLY).expect("resume replays reply");
    let follow_up =
        input_position(&resumed_input, "resumed follow-up").expect("follow-up is present");
    assert!(seed_prompt < seed_reply && seed_reply < follow_up);

    timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_manual_compaction_keeps_provider_binding() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![
            reply("seed", SEED_REPLY),
            reply("compact", "COMPACT_SUMMARY"),
            reply("followup", "FOLLOWUP_REPLY"),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    grok_profile(&server.uri()).write(codex_home.path())?;
    let mut mcp = build_app(codex_home.path()).await?;

    let thread_id = start_grok_thread(&mut mcp).await?;
    assert_eq!(
        send_turn(&mut mcp, &thread_id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    mcp.clear_message_buffer();

    let request = mcp
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: thread_id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    let started = wait_for_context_compaction_started(&mut mcp).await?;
    let completed = wait_for_context_compaction_completed(&mut mcp).await?;
    wait_for_turn_completed(&mut mcp, &started.turn_id).await?;
    let ThreadItem::ContextCompaction { id: started_id } = started.item else {
        unreachable!("started item should be context compaction");
    };
    let ThreadItem::ContextCompaction { id: completed_id } = completed.item else {
        unreachable!("completed item should be context compaction");
    };
    assert_eq!(started_id, completed_id);

    assert_eq!(
        send_turn(&mut mcp, &thread_id, "after compaction").await?,
        TurnStatus::Completed
    );

    let request = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.clone(),
            include_turns: false,
        })
        .await?;
    let ThreadReadResponse { thread } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(
        (thread.model_provider.as_str(), thread.model.as_deref()),
        (GROK_PROVIDER, Some(GROK_MODEL))
    );

    assert_all_requests_are_grok(&mock, 3);
    let follow_up_input = mock.requests()[2].input();
    let summary =
        input_position(&follow_up_input, "COMPACT_SUMMARY").expect("compacted context present");
    let prompt =
        input_position(&follow_up_input, "after compaction").expect("follow-up prompt present");
    assert!(summary < prompt);
    assert!(input_position(&follow_up_input, SEED_REPLY).is_none());

    timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
    Ok(())
}
