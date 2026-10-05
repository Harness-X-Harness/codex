//! Deterministic Grok Provider-binding lifecycle gates for current stock Codex.
//!
//! The product invariant is intentionally expressed through App Server APIs:
//! a thread started with the Grok profile remains bound to that provider and
//! model across ordinary continuation, fork, cold resume, and compaction.
//! These tests do not introduce lifecycle ownership for Grok; they prove that
//! stock durable thread settings remain authoritative.
//!
//! Durable identity is the stock `model_provider` id on the thread/session.
//! Resume, fork, and compaction reconstruct the runtime provider from the
//! current App Server process profile map. `wire_api = "grok_responses"` is
//! the serialized Grok selector; HTTP transport stays stock Responses.

use super::compaction::wait_for_context_compaction_completed;
use super::compaction::wait_for_context_compaction_started;
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

// Only these copied test catalogs disable tools pending C7. Shipped bytes are exercised
// unchanged by grok_model_list and config::grok_catalog_tests, including every capability.
fn write_grok_fixture(home: &std::path::Path, server_uri: &str) -> Result<()> {
    let mut profile: toml::Value = toml::from_str(&std::fs::read_to_string(
        codex_utils_cargo_bin::find_resource!("../../grok/dist/config.toml.example")?,
    )?)?;
    let provider = profile["model_providers"]["grok"]
        .as_table_mut()
        .expect("shipped Grok provider table");
    provider.remove("env_key");
    provider.insert("base_url".into(), format!("{server_uri}/api/codex").into());
    provider.insert("request_max_retries".into(), 0.into());
    provider.insert("stream_max_retries".into(), 0.into());
    let mut catalog: serde_json::Value = serde_json::from_slice(&std::fs::read(
        codex_utils_cargo_bin::find_resource!("../../grok/dist/models.json")?,
    )?)?;
    for model in catalog["models"]
        .as_array_mut()
        .expect("shipped catalog model rows")
    {
        model["shell_type"] = serde_json::json!("disabled");
        model["structured_edit_tool_type"] = serde_json::Value::Null;
        model["node_repl_disabled"] = serde_json::json!(true);
        model["tool_mode"] = serde_json::json!("direct");
    }
    let controls: toml::Value = toml::from_str(
        r#"
approval_policy = "never"
sandbox_mode = "read-only"
web_search = "disabled"
[agents]
enabled = false
[tools.update_plan]
enabled = false
[tools.experimental_request_user_input]
enabled = false
[features]
goals = false
shell_tool = false
view_image = false
sleep_tool = false
multi_agent = false
multi_agent_v2 = false
code_mode = false
apps = false
image_generation = false
tool_suggest = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
"#,
    )?;
    profile
        .as_table_mut()
        .expect("shipped profile table")
        .extend(controls.as_table().expect("fixture controls table").clone());
    let mut stock = profile["model_providers"]["grok"].clone();
    stock["wire_api"] = "responses".into();
    profile["model_providers"]
        .as_table_mut()
        .expect("shipped provider definitions")
        .insert("stock".into(), stock);
    std::fs::write(home.join("config.toml"), toml::to_string(&profile)?)?;
    std::fs::write(home.join("models.json"), serde_json::to_vec(&catalog)?)?;
    Ok(())
}

fn reply(id: &str, text: &str) -> ResponseTemplate {
    let mut added = responses::ev_message_item_added(id, "");
    added["output_index"] = serde_json::json!(0);
    let mut done = responses::ev_assistant_message(id, text);
    done["output_index"] = serde_json::json!(0);
    responses::sse_response(responses::sse(vec![
        responses::ev_response_created(id),
        added,
        serde_json::json!({"type":"response.output_text.delta", "output_index":0,
            "content_index":0, "delta":text}),
        done,
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
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some(GROK_MODEL.into()),
            ..Default::default()
        })
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
    Ok(send_turn_completion(mcp, thread_id, text)
        .await?
        .turn
        .status)
}

async fn send_turn_completion(
    mcp: &mut TestAppServer,
    thread_id: &str,
    text: &str,
) -> Result<TurnCompletedNotification> {
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
        if completed.thread_id == thread_id && completed.turn.id == turn.id {
            return Ok(completed);
        }
    }
}

async fn read_turns(
    mcp: &mut TestAppServer,
    thread_id: &str,
) -> Result<Vec<codex_app_server_protocol::Turn>> {
    let id = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.into(),
            include_turns: true,
        })
        .await?;
    let response: ThreadReadResponse = mcp.read_response(id).await?;
    Ok(response.thread.turns)
}

fn require_public_compaction_history(
    thread: &codex_app_server_protocol::Thread,
    compact_id: &str,
    followup_id: &str,
) -> Result<(usize, usize)> {
    let compact_index = thread
        .turns
        .iter()
        .position(|turn| turn.id == compact_id)
        .ok_or_else(|| anyhow::anyhow!("durable public compaction turn absent"))?;
    let followup_index = thread
        .turns
        .iter()
        .position(|turn| turn.id == followup_id)
        .ok_or_else(|| anyhow::anyhow!("durable public follow-up turn absent"))?;
    anyhow::ensure!(
        compact_index < followup_index,
        "public compaction/follow-up order changed"
    );
    let compact = &thread.turns[compact_index];
    anyhow::ensure!(
        compact.status == TurnStatus::Completed && compact.error.is_none(),
        "public compaction did not complete"
    );
    // Reconstruction owns item IDs and exposes a marker, not private summary text.
    anyhow::ensure!(
        compact
            .items
            .iter()
            .any(|item| matches!(item, ThreadItem::ContextCompaction { .. })),
        "public compaction marker absent"
    );
    let followup = &thread.turns[followup_index];
    anyhow::ensure!(
        followup.status == TurnStatus::Completed && followup.error.is_none(),
        "public follow-up did not complete"
    );
    anyhow::ensure!(
        followup.items.iter().any(
            |item| matches!(item, ThreadItem::AgentMessage { text, .. } if text == "FOLLOWUP_REPLY")
        ),
        "bound public follow-up reply absent"
    );
    anyhow::ensure!(followup.items.iter().any(|item| matches!(item, ThreadItem::UserMessage { content, .. } if content.iter().any(|input| matches!(input, V2UserInput::Text { text, .. } if text == "after compaction")))), "bound public follow-up input absent");
    Ok((compact_index, followup_index))
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
        let body = request.body_json();
        assert_eq!(body["model"], GROK_MODEL);
        assert!(body.get("tools").is_none());
        assert!(body.get("store").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_normal_continuation_keeps_provider_binding() -> Result<()> {
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
    write_grok_fixture(codex_home.path(), &server.uri())?;
    let mut mcp = build_app(codex_home.path()).await?;

    let thread_id = start_grok_thread(&mut mcp).await?;
    assert_eq!(
        send_turn(&mut mcp, &thread_id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let other = responses::start_mock_server().await;
    let profile_path = codex_home.path().join("config.toml");
    let profile = std::fs::read_to_string(&profile_path)?.replace(&server.uri(), &other.uri());
    std::fs::write(profile_path, profile)?;
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
    assert!(other.received_requests().await.unwrap().is_empty());
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
    write_grok_fixture(codex_home.path(), &server.uri())?;
    let mut mcp = build_app(codex_home.path()).await?;

    let source_id = start_grok_thread(&mut mcp).await?;
    assert_eq!(
        send_turn(&mut mcp, &source_id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );

    let seed_turns = read_turns(&mut mcp, &source_id).await?;
    assert_eq!(seed_turns.len(), 1);
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

    let source_turns = read_turns(&mut mcp, &source_id).await?;
    let fork_turns = read_turns(&mut mcp, &fork.id).await?;
    assert_eq!(&source_turns[..seed_turns.len()], seed_turns.as_slice());
    assert_eq!(&fork_turns[..seed_turns.len()], seed_turns.as_slice());
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
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(&server, vec![reply("seed", SEED_REPLY)]).await;
    let codex_home = TempDir::new()?;
    write_grok_fixture(codex_home.path(), &server.uri())?;
    let grok_config = std::fs::read_to_string(codex_home.path().join("config.toml"))?;

    let (thread_id, seed_turns) = {
        let mut mcp = build_app(codex_home.path()).await?;
        let thread_id = start_grok_thread(&mut mcp).await?;
        assert_eq!(
            send_turn(&mut mcp, &thread_id, SEED_PROMPT).await?,
            TurnStatus::Completed
        );
        let seed_turns = read_turns(&mut mcp, &thread_id).await?;
        timeout(DEFAULT_READ_TIMEOUT, mcp.shutdown_gracefully()).await??;
        (thread_id, seed_turns)
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

    let current_gateway = responses::start_mock_server().await;
    let current_requests = responses::mount_response_sequence(
        &current_gateway,
        vec![reply("resumed", "RESUMED_REPLY")],
    )
    .await;
    std::fs::write(
        codex_home.path().join("config.toml"),
        grok_config.replace(&server.uri(), &current_gateway.uri()),
    )?;
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

    let resumed_turns = read_turns(&mut mcp, &thread_id).await?;
    assert_eq!(&resumed_turns[..seed_turns.len()], seed_turns.as_slice());
    assert_all_requests_are_grok(&mock, 1);
    assert_all_requests_are_grok(&current_requests, 1);
    let resumed_input = current_requests.requests()[0].input();
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
    write_grok_fixture(codex_home.path(), &server.uri())?;
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
    assert_eq!(
        (&started.thread_id, &completed.thread_id, &completed.turn_id),
        (&thread_id, &thread_id, &started.turn_id)
    );
    let terminal: TurnCompletedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_notification("turn/completed"),
    )
    .await??;
    assert_eq!(
        (
            &terminal.thread_id,
            &terminal.turn.id,
            &terminal.turn.status,
            &terminal.turn.error
        ),
        (&thread_id, &started.turn_id, &TurnStatus::Completed, &None)
    );
    let ThreadItem::ContextCompaction { id: started_id } = started.item else {
        unreachable!("started item should be context compaction");
    };
    let ThreadItem::ContextCompaction { id: completed_id } = completed.item else {
        unreachable!("completed item should be context compaction");
    };
    assert_eq!(started_id, completed_id);

    let followup_terminal = send_turn_completion(&mut mcp, &thread_id, "after compaction").await?;
    assert_eq!(followup_terminal.turn.status, TurnStatus::Completed);

    let request = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.clone(),
            include_turns: true,
        })
        .await?;
    let ThreadReadResponse { thread } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(
        (thread.model_provider.as_str(), thread.model.as_deref()),
        (GROK_PROVIDER, Some(GROK_MODEL))
    );

    let (compact_index, followup_index) =
        require_public_compaction_history(&thread, &started.turn_id, &followup_terminal.turn.id)?;
    // Exercise the public-history oracle with bounded corruptions of the real result.
    for corruption in [
        "missing_marker",
        "missing_turn",
        "reordered",
        "failed_compaction",
        "failed_followup",
        "unrelated_followup",
    ] {
        let mut corrupted = thread.clone();
        match corruption {
            "missing_marker" => corrupted.turns[compact_index]
                .items
                .retain(|item| !matches!(item, ThreadItem::ContextCompaction { .. })),
            "missing_turn" => {
                corrupted.turns.remove(compact_index);
            }
            "reordered" => corrupted.turns.swap(compact_index, followup_index),
            "failed_compaction" => corrupted.turns[compact_index].status = TurnStatus::Failed,
            "failed_followup" => corrupted.turns[followup_index].status = TurnStatus::Failed,
            "unrelated_followup" => corrupted.turns[followup_index].id = "unrelated".into(),
            _ => unreachable!(),
        }
        assert!(
            require_public_compaction_history(
                &corrupted,
                &started.turn_id,
                &followup_terminal.turn.id
            )
            .is_err(),
            "{corruption}"
        );
    }

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_cold_fork_preserves_pinned_model_under_different_default() -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_response_sequence(
        &server,
        vec![reply("seed", SEED_REPLY), reply("fork", "COLD_FORK_REPLY")],
    )
    .await;
    let home = TempDir::new()?;
    write_grok_fixture(home.path(), &server.uri())?;
    let mut app = build_app(home.path()).await?;
    let source = start_grok_thread(&mut app).await?;
    assert_eq!(
        send_turn(&mut app, &source, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    app.shutdown_gracefully().await?;
    let mut app = build_app(home.path()).await?;
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source.clone(),
            ..Default::default()
        })
        .await?;
    let fork: ThreadForkResponse = app.read_response(id).await?;
    assert_eq!(
        (
            fork.model.as_str(),
            fork.model_provider.as_str(),
            fork.thread.model_provider.as_str()
        ),
        (GROK_MODEL, GROK_PROVIDER, GROK_PROVIDER)
    );
    assert_ne!(fork.thread.id, source);
    assert_eq!(
        send_turn(&mut app, &fork.thread.id, "cold fork continuation").await?,
        TurnStatus::Completed
    );
    assert_all_requests_are_grok(&requests, 2);
    assert!(requests.requests()[1].body_contains_text(SEED_REPLY));
    app.shutdown_gracefully().await?;
    Ok(())
}

#[test_case::test_case("typed_model"; "typed_model")]
#[test_case::test_case("typed_provider"; "typed_provider")]
#[test_case::test_case("model"; "config_model")]
#[test_case::test_case("model_provider"; "config_provider")]
#[test_case::test_case("model_reasoning_effort"; "config_effort")]
#[test_case::test_case("model_catalog_json"; "config_catalog")]
#[test_case::test_case("model_providers.grok.name"; "dotted_provider_definition")]
#[test_case::test_case("model_providers"; "nested_provider_definition")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_fork_explicit_model_policy_retains_request_precedence(selector: &str) -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_response_sequence(
        &server,
        vec![reply("seed", SEED_REPLY), reply("fork", "OVERRIDDEN_REPLY")],
    )
    .await;
    let home = TempDir::new()?;
    write_grok_fixture(home.path(), &server.uri())?;
    let mut app = build_app(home.path()).await?;
    let source = start_grok_thread(&mut app).await?;
    assert_eq!(
        send_turn(&mut app, &source, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let mut params = ThreadForkParams {
        thread_id: source,
        ..Default::default()
    };
    let mut expected_provider = GROK_PROVIDER;
    match selector {
        "typed_model" => params.model = Some("grok-4.7".into()),
        "typed_provider" => {
            params.model_provider = Some("stock".into());
            expected_provider = "stock";
        }
        key => {
            let value = match key {
                "model" => serde_json::json!("grok-4.7"),
                "model_provider" => {
                    expected_provider = "stock";
                    serde_json::json!("stock")
                }
                "model_reasoning_effort" => serde_json::json!("low"),
                "model_catalog_json" => serde_json::json!(home.path().join("models.json")),
                "model_providers.grok.name" => serde_json::json!("Explicit alias"),
                "model_providers" => serde_json::json!({"grok":{"name":"Explicit alias"}}),
                _ => unreachable!(),
            };
            params.config = Some([(key.into(), value)].into());
        }
    }
    let id = app.send_thread_fork_request(params).await?;
    let fork: ThreadForkResponse = app.read_response(id).await?;
    assert_eq!(
        (fork.model.as_str(), fork.model_provider.as_str()),
        ("grok-4.7", expected_provider)
    );
    assert_eq!(
        send_turn(&mut app, &fork.thread.id, "explicit fork continuation").await?,
        TurnStatus::Completed
    );
    let outbound = requests.requests();
    assert_eq!(outbound.len(), 2);
    assert_eq!(outbound[1].body_json()["model"], "grok-4.7");
    assert_eq!(
        outbound[1].body_json().get("store").is_none(),
        expected_provider == GROK_PROVIDER
    );
    app.shutdown_gracefully().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_fork_rejects_conflicting_managed_provider_then_recovers() -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_response_sequence(
        &server,
        vec![reply("seed", SEED_REPLY), reply("fork", "RECOVERED_REPLY")],
    )
    .await;
    let home = TempDir::new()?;
    write_grok_fixture(home.path(), &server.uri())?;
    let mut app = build_app(home.path()).await?;
    let source = start_grok_thread(&mut app).await?;
    assert_eq!(
        send_turn(&mut app, &source, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let requirements = home.path().join("requirements.toml");
    std::fs::write(&requirements, "model_provider = \"stock\"\n")?;
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source.clone(),
            ..Default::default()
        })
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert!(
        error
            .error
            .message
            .contains("does not permit the source thread's Grok provider")
    );
    assert_eq!(requests.requests().len(), 1);
    std::fs::write(requirements, "")?;
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source,
            ..Default::default()
        })
        .await?;
    let fork: ThreadForkResponse = app.read_response(id).await?;
    assert_eq!(
        (fork.model.as_str(), fork.model_provider.as_str()),
        (GROK_MODEL, GROK_PROVIDER)
    );
    assert_eq!(
        send_turn(&mut app, &fork.thread.id, "recovered fork").await?,
        TurnStatus::Completed
    );
    assert_all_requests_are_grok(&requests, 2);
    app.shutdown_gracefully().await?;
    Ok(())
}

#[test_case::test_case(false; "valid_source_profile")]
#[test_case::test_case(true; "unresolved_source_profile")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stock_fork_keeps_current_default_and_accepts_unlisted_model(
    unresolved: bool,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let mut replies = vec![reply("seed", SEED_REPLY), reply("fork", "STOCK_REPLY")];
    if !unresolved {
        replies.push(reply("source", "STOCK_SOURCE_REPLY"));
    }
    let requests = responses::mount_response_sequence(&server, replies).await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model("unlisted-stock-default")
        .write(home.path())?;
    let mut app = build_app(home.path()).await?;
    let id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("unlisted-stock-selected".into()),
            ..Default::default()
        })
        .await?;
    let source: ThreadStartResponse = app.read_response(id).await?;
    assert_eq!(
        send_turn(&mut app, &source.thread.id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let expected_provider = if unresolved {
        app.shutdown_gracefully().await?;
        MockResponsesConfig::new(&server.uri())
            .with_model_provider("replacement-stock")
            .with_model("unlisted-stock-default")
            .write(home.path())?;
        app = build_app(home.path()).await?;
        "replacement-stock"
    } else {
        "mock_provider"
    };
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source.thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let fork: ThreadForkResponse = app.read_response(id).await?;
    assert_eq!(
        (fork.model.as_str(), fork.model_provider.as_str()),
        ("unlisted-stock-default", expected_provider)
    );
    assert_eq!(
        send_turn(&mut app, &fork.thread.id, "stock fork").await?,
        TurnStatus::Completed
    );
    if !unresolved {
        assert_eq!(
            send_turn(&mut app, &source.thread.id, "stock source").await?,
            TurnStatus::Completed
        );
    }
    assert_eq!(
        requests
            .requests()
            .iter()
            .map(|request| request.body_json()["model"]
                .as_str()
                .expect("captured request model string")
                .to_owned())
            .collect::<Vec<_>>(),
        if unresolved {
            vec!["unlisted-stock-selected", "unlisted-stock-default"]
        } else {
            vec![
                "unlisted-stock-selected",
                "unlisted-stock-default",
                "unlisted-stock-selected",
            ]
        }
    );
    app.shutdown_gracefully().await?;
    Ok(())
}

#[test_case::test_case("custom-route", "grok_responses", "grok-4.6"; "explicit_dialect_alias")]
#[test_case::test_case("grok", "responses", "grok-4.7"; "stock_grok_name")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_retention_uses_explicit_dialect_not_provider_id_or_name(
    provider_id: &str,
    wire_api: &str,
    expected_model: &str,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_response_sequence(
        &server,
        vec![reply("seed", SEED_REPLY), reply("fork", "ALIAS_REPLY")],
    )
    .await;
    let home = TempDir::new()?;
    write_grok_fixture(home.path(), &server.uri())?;
    let path = home.path().join("config.toml");
    let mut profile: toml::Value = toml::from_str(&std::fs::read_to_string(&path)?)?;
    let mut provider = profile["model_providers"]
        .as_table_mut()
        .expect("shipped provider definitions")
        .remove("grok")
        .expect("shipped Grok provider definition");
    provider["wire_api"] = wire_api.into();
    profile["model_providers"]
        .as_table_mut()
        .expect("shipped provider definitions")
        .insert(provider_id.into(), provider);
    profile["model_provider"] = provider_id.into();
    std::fs::write(path, toml::to_string(&profile)?)?;
    let mut app = build_app(home.path()).await?;
    let id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some(GROK_MODEL.into()),
            ..Default::default()
        })
        .await?;
    let source: ThreadStartResponse = app.read_response(id).await?;
    assert_eq!(
        send_turn(&mut app, &source.thread.id, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source.thread.id,
            ..Default::default()
        })
        .await?;
    let fork: ThreadForkResponse = app.read_response(id).await?;
    assert_eq!(
        (fork.model.as_str(), fork.model_provider.as_str()),
        (expected_model, provider_id)
    );
    assert_eq!(
        send_turn(&mut app, &fork.thread.id, "alias fork").await?,
        TurnStatus::Completed
    );
    let outbound = requests.requests();
    assert_eq!(outbound.len(), 2);
    assert_eq!(outbound[1].body_json()["model"], expected_model);
    assert_eq!(
        outbound[1].body_json().get("store").is_none(),
        wire_api == "grok_responses"
    );
    app.shutdown_gracefully().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_fork_does_not_hide_invalid_legacy_profile_override() -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests =
        responses::mount_response_sequence(&server, vec![reply("seed", SEED_REPLY)]).await;
    let home = TempDir::new()?;
    write_grok_fixture(home.path(), &server.uri())?;
    let mut app = build_app(home.path()).await?;
    let source = start_grok_thread(&mut app).await?;
    assert_eq!(
        send_turn(&mut app, &source, SEED_PROMPT).await?,
        TurnStatus::Completed
    );
    let id = app
        .send_thread_fork_request(ThreadForkParams {
            thread_id: source,
            config: Some([("profile".into(), serde_json::json!("legacy"))].into()),
            ..Default::default()
        })
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert!(error.error.message.contains("legacy"));
    assert_eq!(requests.requests().len(), 1);
    app.shutdown_gracefully().await?;
    Ok(())
}
