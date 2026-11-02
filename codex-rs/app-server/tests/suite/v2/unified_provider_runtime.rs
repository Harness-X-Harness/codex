//! One real app-server process, one configuration, two independently gated HTTP providers.
//! The gates prove overlapping inference and deliberately interleaved public output without
//! relying on sleeps, event counts, paid endpoints, or credentials from the test machine.

use anyhow::Context;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::ModelProviderError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::login_with_api_key;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::StreamingSseServer;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(60);
const OPENAI: &str = "openai";
const GROK: &str = "grok";
const OPENAI_MODEL: &str = "gpt-5.4";
const GROK_MODEL: &str = "grok-4.6";
const OPENAI_CATALOG_MARKER: &str = "OPENAI_CATALOG_ONLY";
const GROK_CATALOG_MARKER: &str = "GROK_CATALOG_ONLY";
const OPENAI_KEY: &str = "test-openai-key-not-a-secret";
const GROK_KEY: &str = "test-grok-key-not-a-secret";
const OPENAI_ACCOUNT_ID: &str = "test-openai-chatgpt-account";
const CIPHERTEXT: &str = "opaque-grok-history-must-not-reach-openai";

fn model(provider: &str) -> &'static str {
    if provider == OPENAI {
        OPENAI_MODEL
    } else {
        GROK_MODEL
    }
}

/// Use the existing tool-free fixture controls, but define both providers in this single
/// config. The process-default catalog deliberately has different metadata for GROK_MODEL
/// than grok-models.json, so a shared startup ModelsManager cannot accidentally pass.
fn write_fixture(home: &Path, openai: &str, grok: &str) -> Result<()> {
    super::grok_provider_binding::write_grok_fixture(home, grok)?;
    let config_path = home.join("config.toml");
    let mut config: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
    let mut grok_provider = config["model_providers"][GROK]
        .as_table()
        .context("Grok provider table")?
        .clone();
    grok_provider.insert("base_url".into(), format!("{grok}/v1").into());
    // write_grok_fixture deliberately removes env_key; TOML mutable indexing does
    // not insert absent keys, unlike serde_json::Value indexing below.
    grok_provider.insert("env_key".into(), "UNIFIED_TEST_GROK_KEY".into());
    let config_table = config
        .as_table_mut()
        .context("fixture configuration table")?;
    // Built-in IDs are intentionally not overridable through model_providers. Use
    // the supported OpenAI endpoint override, retaining the real built-in provider.
    config_table.insert(
        "model_providers".into(),
        toml::Value::Table(
            [(GROK.into(), toml::Value::Table(grok_provider))]
                .into_iter()
                .collect(),
        ),
    );
    config_table.insert("openai_base_url".into(), format!("{openai}/v1").into());
    config_table.insert("cli_auth_credentials_store".into(), "file".into());
    let features = config_table
        .get_mut("features")
        .and_then(toml::Value::as_table_mut)
        .context("fixture features table")?;
    features.insert("enable_request_compression".into(), false.into());
    features.insert("api_key_model_discovery".into(), false.into());
    config_table.insert("model_provider".into(), OPENAI.into());
    config_table.insert("model".into(), OPENAI_MODEL.into());
    std::fs::write(config_path, toml::to_string(&config)?)?;
    login_with_api_key(
        home,
        OPENAI_KEY,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?;

    let source: Value = serde_json::from_slice(&std::fs::read(home.join("models.json"))?)?;
    let mut openai_model = source["models"][0].clone();
    openai_model["slug"] = json!(OPENAI_MODEL);
    openai_model["base_instructions"] = json!(OPENAI_CATALOG_MARKER);
    openai_model["supports_reasoning_summary_parameter"] = json!(true);
    let mut grok_model = source["models"][1].clone();
    grok_model["base_instructions"] = json!(GROK_CATALOG_MARKER);
    std::fs::write(
        home.join("grok-models.json"),
        serde_json::to_vec(&json!({"models": [grok_model.clone()]}))?,
    )?;
    grok_model["base_instructions"] = json!("WRONG_PROCESS_DEFAULT_GROK_CATALOG");
    std::fs::write(
        home.join("models.json"),
        serde_json::to_vec(&json!({"models": [openai_model, grok_model]}))?,
    )?;
    Ok(())
}

async fn app(home: &Path) -> Result<TestAppServer> {
    TestAppServer::builder()
        .with_codex_home(home)
        .with_env_overrides(&[
            ("UNIFIED_TEST_GROK_KEY", Some(GROK_KEY)),
            ("OPENAI_API_KEY", None),
            ("CODEX_API_KEY", None),
            ("OPENAI_BASE_URL", None),
        ])
        .build_initialized_with_timeout(WAIT)
        .await
}

async fn start(app: &mut TestAppServer, home: &Path, provider: &str) -> Result<String> {
    let response = app
        .start_thread(ThreadStartParams {
            model: Some(model(provider).into()),
            model_provider: Some(provider.into()),
            config: (provider == GROK).then(|| {
                [(
                    "model_catalog_json".into(),
                    json!(home.join("grok-models.json")),
                )]
                .into()
            }),
            ..Default::default()
        })
        .await?;
    assert_eq!(response.model_provider, provider);
    assert_eq!(response.thread.model_provider, provider);
    assert_eq!(response.model, model(provider));
    Ok(response.thread.id)
}

async fn begin(app: &mut TestAppServer, thread: &str, text: &str) -> Result<String> {
    let id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.into(),
            input: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let response: TurnStartResponse = timeout(WAIT, app.read_response(id)).await??;
    Ok(response.turn.id)
}

async fn completed(
    app: &mut TestAppServer,
    thread: &str,
    turn: &str,
    status: TurnStatus,
) -> Result<()> {
    let notification = timeout(
        WAIT,
        app.read_stream_until_matching_notification("matching provider turn completion", |n| {
            n.method == "turn/completed"
                && n.params
                    .as_ref()
                    .is_some_and(|p| p["threadId"] == thread && p["turn"]["id"] == turn)
        }),
    )
    .await??;
    let notification: TurnCompletedNotification =
        serde_json::from_value(notification.params.context("turn completion params")?)?;
    assert_eq!(notification.turn.status, status);
    assert_eq!(notification.turn.error, None);
    Ok(())
}

async fn delta(app: &mut TestAppServer, thread: &str, turn: &str, text: &str) -> Result<()> {
    timeout(
        WAIT,
        app.read_stream_until_matching_notification("matching provider text delta", |n| {
            n.method == "item/agentMessage/delta"
                && n.params.as_ref().is_some_and(|p| {
                    p["threadId"] == thread && p["turnId"] == turn && p["delta"] == text
                })
        }),
    )
    .await??;
    Ok(())
}

async fn assistant_texts(app: &mut TestAppServer, thread: &str) -> Result<Vec<String>> {
    let id = app
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread.into(),
            include_turns: true,
        })
        .await?;
    let response: ThreadReadResponse = timeout(WAIT, app.read_response(id)).await??;
    Ok(response
        .thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter_map(|item| match &item.payload {
            ThreadItemPayload::AgentMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect())
}

fn added(id: &str) -> Value {
    let mut event = responses::ev_message_item_added(id, "");
    event["output_index"] = json!(0);
    event
}

fn done(id: &str, text: &str) -> Value {
    let mut event = responses::ev_assistant_message(id, text);
    event["output_index"] = json!(0);
    event
}

fn text_delta(text: &str) -> Value {
    json!({"type": "response.output_text.delta", "output_index": 0, "content_index": 0, "delta": text})
}

fn chunk(events: Vec<Value>, gate: Option<oneshot::Receiver<()>>) -> StreamingSseChunk {
    StreamingSseChunk {
        gate,
        body: responses::sse(events),
    }
}

fn reply(id: &str, text: &str) -> Vec<StreamingSseChunk> {
    vec![chunk(
        vec![
            responses::ev_response_created(id),
            added(id),
            text_delta(text),
            done(id, text),
            responses::ev_completed(id),
        ],
        None,
    )]
}

fn gated_reply(
    id: &str,
    first: &str,
    last: &str,
) -> (
    Vec<StreamingSseChunk>,
    oneshot::Sender<()>,
    oneshot::Sender<()>,
) {
    let (release_first, first_gate) = oneshot::channel();
    let (release_last, last_gate) = oneshot::channel();
    (
        vec![
            chunk(vec![responses::ev_response_created(id), added(id)], None),
            chunk(vec![text_delta(first)], Some(first_gate)),
            chunk(
                vec![
                    text_delta(last),
                    done(id, &format!("{first}{last}")),
                    responses::ev_completed(id),
                ],
                Some(last_gate),
            ),
        ],
        release_first,
        release_last,
    )
}

async fn request_bodies(server: &StreamingSseServer) -> Result<Vec<Value>> {
    server
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice(body).map_err(Into::into))
        .collect()
}

async fn assert_route(server: &StreamingSseServer, provider: &str, count: usize) -> Result<()> {
    let requests = request_bodies(server).await?;
    assert_eq!(requests.len(), count);
    for body in requests {
        assert_eq!(body["model"], model(provider));
        assert_eq!(body["store"], false);
        // The Grok request projection must not leak OpenAI-only fields.
        assert_eq!(body.get("parallel_tool_calls").is_none(), provider == GROK);
        // Stock Responses carries catalog instructions in its first developer
        // message, not a top-level `instructions` field (see instructions_text()).
        let instructions = body["input"]
            .as_array()
            .context("request input array")?
            .iter()
            .find(|item| item["type"] == "message" && item["role"] == "developer")
            .context("base instructions developer message")?["content"]
            .as_array()
            .context("developer message content")?
            .iter()
            .filter(|item| item["type"] == "input_text")
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        let (own, other) = if provider == OPENAI {
            (OPENAI_CATALOG_MARKER, GROK_CATALOG_MARKER)
        } else {
            (GROK_CATALOG_MARKER, OPENAI_CATALOG_MARKER)
        };
        assert!(
            instructions.contains(own),
            "missing provider-local catalog instructions: {body}"
        );
        assert!(!instructions.contains(other));
        assert!(!instructions.contains("WRONG_PROCESS_DEFAULT_GROK_CATALOG"));
    }
    let headers = server.request_headers().await;
    assert_eq!(headers.len(), count);
    let (own, other) = if provider == OPENAI {
        (OPENAI_KEY, GROK_KEY)
    } else {
        (GROK_KEY, OPENAI_KEY)
    };
    for headers in headers {
        assert!(headers.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        let auth = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim())
        });
        assert_eq!(auth, Some(format!("Bearer {own}").as_str()));
        assert!(!headers.contains(other));
        if provider == GROK {
            assert!(!headers.contains(OPENAI_ACCOUNT_ID));
            assert!(
                !headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .any(|(name, _)| {
                        name.eq_ignore_ascii_case("chatgpt-account-id")
                            || name.eq_ignore_ascii_case("x-oai-attestation")
                    })
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_runtime_interleaves_provider_streams_with_isolated_catalogs_and_auth() -> Result<()>
{
    let (openai_stream, openai_first, openai_last) = gated_reply("oa", "OPENAI_FIRST", "_LAST");
    let (grok_stream, grok_first, grok_last) = gated_reply("gr", "GROK_FIRST", "_LAST");
    let (openai, _) = start_streaming_sse_server(vec![openai_stream]).await;
    let (grok, _) = start_streaming_sse_server(vec![grok_stream]).await;
    let home = TempDir::new()?;
    write_fixture(home.path(), openai.uri(), grok.uri())?;
    // Use the real built-in OpenAI provider with synthetic ChatGPT identity. The
    // harness routes account/bootstrap calls to its local mock backend automatically;
    // Grok must not inherit this token, account, or attestation authority.
    write_chatgpt_auth(
        home.path(),
        ChatGptAuthFixture::new(OPENAI_KEY)
            .account_id(OPENAI_ACCOUNT_ID)
            .chatgpt_account_id(OPENAI_ACCOUNT_ID)
            .plan_type("plus"),
        AuthCredentialsStoreMode::File,
    )?;
    let mut app = app(home.path()).await?;
    let oa_thread = start(&mut app, home.path(), OPENAI).await?;
    let gr_thread = start(&mut app, home.path(), GROK).await?;
    let oa_turn = begin(&mut app, &oa_thread, "openai isolated input").await?;
    let gr_turn = begin(&mut app, &gr_thread, "grok isolated input").await?;

    // Neither endpoint can complete until its own gate is released. Observing both
    // requests here proves concurrent in-flight turns, not sequential fast responses.
    timeout(WAIT, openai.wait_for_request_count(1)).await?;
    timeout(WAIT, grok.wait_for_request_count(1)).await?;
    openai_first.send(()).expect("release OpenAI first chunk");
    delta(&mut app, &oa_thread, &oa_turn, "OPENAI_FIRST").await?;
    grok_first.send(()).expect("release Grok first chunk");
    delta(&mut app, &gr_thread, &gr_turn, "GROK_FIRST").await?;
    openai_last.send(()).expect("release OpenAI final chunk");
    completed(&mut app, &oa_thread, &oa_turn, TurnStatus::Completed).await?;
    grok_last.send(()).expect("release Grok final chunk");
    completed(&mut app, &gr_thread, &gr_turn, TurnStatus::Completed).await?;

    assert_eq!(
        assistant_texts(&mut app, &oa_thread).await?,
        vec!["OPENAI_FIRST_LAST"]
    );
    assert_eq!(
        assistant_texts(&mut app, &gr_thread).await?,
        vec!["GROK_FIRST_LAST"]
    );
    assert_route(&openai, OPENAI, 1).await?;
    assert_route(&grok, GROK, 1).await?;
    let oa_body = request_bodies(&openai).await?[0].to_string();
    let gr_body = request_bodies(&grok).await?[0].to_string();
    assert!(oa_body.contains("openai isolated input") && !oa_body.contains("grok isolated input"));
    assert!(gr_body.contains("grok isolated input") && !gr_body.contains("openai isolated input"));
    app.shutdown_gracefully().await?;
    openai.shutdown().await;
    grok.shutdown().await;
    Ok(())
}

#[test_case::test_case(OPENAI; "cancel_openai")]
#[test_case::test_case(GROK; "cancel_grok")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_runtime_default_change_and_cancel_remain_thread_local(cancel: &str) -> Result<()> {
    let (oa_stream, oa_first, oa_last) = gated_reply("oa-cancel", "OPENAI_ACTIVE", "_SURVIVED");
    let (gr_stream, gr_first, gr_last) = gated_reply("gr-cancel", "GROK_ACTIVE", "_SURVIVED");
    let (openai, _) =
        start_streaming_sse_server(vec![oa_stream, reply("oa-next", "OPENAI_NEXT")]).await;
    let (grok, _) =
        start_streaming_sse_server(vec![gr_stream, reply("gr-next", "GROK_NEXT")]).await;
    let home = TempDir::new()?;
    write_fixture(home.path(), openai.uri(), grok.uri())?;
    let mut app = app(home.path()).await?;
    let oa_thread = start(&mut app, home.path(), OPENAI).await?;
    let gr_thread = start(&mut app, home.path(), GROK).await?;
    let oa_turn = begin(&mut app, &oa_thread, "openai pending").await?;
    let gr_turn = begin(&mut app, &gr_thread, "grok pending").await?;
    timeout(WAIT, openai.wait_for_request_count(1)).await?;
    timeout(WAIT, grok.wait_for_request_count(1)).await?;
    oa_first.send(()).expect("release OpenAI first chunk");
    gr_first.send(()).expect("release Grok first chunk");
    delta(&mut app, &oa_thread, &oa_turn, "OPENAI_ACTIVE").await?;
    delta(&mut app, &gr_thread, &gr_turn, "GROK_ACTIVE").await?;

    let path = home.path().join("config.toml");
    let mut config: toml::Value = toml::from_str(&std::fs::read_to_string(&path)?)?;
    config["model_provider"] = GROK.into();
    config["model"] = GROK_MODEL.into();
    std::fs::write(path, toml::to_string(&config)?)?;
    let (
        canceled_thread,
        canceled_turn,
        survivor_thread,
        survivor_turn,
        survivor_last,
        canceled_last,
    ) = if cancel == OPENAI {
        (&oa_thread, &oa_turn, &gr_thread, &gr_turn, gr_last, oa_last)
    } else {
        (&gr_thread, &gr_turn, &oa_thread, &oa_turn, oa_last, gr_last)
    };
    let id = app
        .send_turn_interrupt_request(TurnInterruptParams {
            thread_id: canceled_thread.clone(),
            turn_id: canceled_turn.clone(),
        })
        .await?;
    let _: TurnInterruptResponse = timeout(WAIT, app.read_response(id)).await??;
    completed(
        &mut app,
        canceled_thread,
        canceled_turn,
        TurnStatus::Interrupted,
    )
    .await?;
    // The sibling stream is still independently gated, then succeeds after cancellation.
    survivor_last.send(()).expect("release uncanceled provider");
    completed(
        &mut app,
        survivor_thread,
        survivor_turn,
        TurnStatus::Completed,
    )
    .await?;
    drop(canceled_last);

    // A changed process default must neither reject nor reroute either existing thread.
    for (thread, prompt) in [
        (&oa_thread, "openai after default change"),
        (&gr_thread, "grok after default change"),
    ] {
        let turn = begin(&mut app, thread, prompt).await?;
        completed(&mut app, thread, &turn, TurnStatus::Completed).await?;
    }
    let oa_texts = assistant_texts(&mut app, &oa_thread).await?;
    let gr_texts = assistant_texts(&mut app, &gr_thread).await?;
    assert_eq!(oa_texts.last().map(String::as_str), Some("OPENAI_NEXT"));
    assert_eq!(gr_texts.last().map(String::as_str), Some("GROK_NEXT"));
    let (survivor_texts, survivor_expected) = if cancel == OPENAI {
        (&gr_texts, "GROK_ACTIVE_SURVIVED")
    } else {
        (&oa_texts, "OPENAI_ACTIVE_SURVIVED")
    };
    assert!(survivor_texts.iter().any(|text| text == survivor_expected));
    assert!(oa_texts.iter().all(|text| !text.contains("GROK")));
    assert!(gr_texts.iter().all(|text| !text.contains("OPENAI")));
    assert_route(&openai, OPENAI, 2).await?;
    assert_route(&grok, GROK, 2).await?;
    app.shutdown_gracefully().await?;
    openai.shutdown().await;
    grok.shutdown().await;
    Ok(())
}

fn encrypted_reply() -> Vec<StreamingSseChunk> {
    let reasoning = json!({"type": "reasoning", "id": "private-grok-reasoning", "summary": [], "encrypted_content": CIPHERTEXT});
    vec![chunk(
        vec![
            responses::ev_response_created("seed"),
            added("seed"),
            text_delta("GROK_SEED"),
            done("seed", "GROK_SEED"),
            json!({"type": "response.output_item.added", "output_index": 1, "item": reasoning}),
            json!({"type": "response.output_item.done", "output_index": 1, "item": reasoning}),
            responses::ev_completed("seed"),
        ],
        None,
    )]
}

async fn expect_provider_error(
    app: &mut TestAppServer,
    id: i64,
    expected: &ModelProviderError,
) -> Result<()> {
    let error = timeout(
        WAIT,
        app.read_stream_until_error_message(RequestId::Integer(id)),
    )
    .await??;
    assert_eq!(error.error.code, -32600);
    assert_eq!(error.error.data, Some(serde_json::to_value(expected)?));
    Ok(())
}

#[test_case::test_case(false; "loaded_thread")]
#[test_case::test_case(true; "cold_resume")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_runtime_resume_retains_provider_and_never_mixes_encrypted_history(
    cold: bool,
) -> Result<()> {
    let (openai, _) = start_streaming_sse_server(vec![]).await;
    let mut replies = vec![encrypted_reply()];
    if !cold {
        replies.push(reply("config-rejoin", "GROK_AFTER_CONFIG_OVERRIDE"));
        replies.push(reply("typed-rejoin", "GROK_AFTER_TYPED_OVERRIDE"));
    }
    replies.push(reply("continued", "GROK_RESUMED"));
    let (grok, _) = start_streaming_sse_server(replies).await;
    let home = TempDir::new()?;
    write_fixture(home.path(), openai.uri(), grok.uri())?;
    let mut app = app(home.path()).await?;
    let thread = start(&mut app, home.path(), GROK).await?;
    let turn = begin(&mut app, &thread, "grok seed private history").await?;
    completed(&mut app, &thread, &turn, TurnStatus::Completed).await?;
    if cold {
        app.shutdown_gracefully().await?;
        app = self::app(home.path()).await?;
    }
    for typed in [false, true] {
        let mut params = ThreadResumeParams {
            thread_id: thread.clone(),
            ..Default::default()
        };
        if typed {
            params.model_provider = Some(OPENAI.into());
        } else {
            params.config = Some([("model_provider".into(), json!(OPENAI))].into());
        }
        let id = app.send_thread_resume_request(params).await?;
        if cold {
            expect_provider_error(
                &mut app,
                id,
                &ModelProviderError::ModelProviderMismatch {
                    model_provider: GROK.into(),
                    current_model_provider: OPENAI.into(),
                },
            )
            .await?;
        } else {
            // Loaded, subscribed threads preserve the existing rejoin contract: overrides
            // are ignored rather than rebuilding a session another client can observe.
            let rejoined: ThreadResumeResponse = timeout(WAIT, app.read_response(id)).await??;
            assert_eq!(rejoined.model_provider, GROK);
            assert_eq!(rejoined.thread.model_provider, GROK);
            // Exercise inference immediately: a later normal resume must not be able
            // to hide an override that temporarily changed the running session route.
            let turn = begin(
                &mut app,
                &thread,
                "continue immediately after ignored override",
            )
            .await?;
            completed(&mut app, &thread, &turn, TurnStatus::Completed).await?;
            assert!(openai.requests().await.is_empty());
        }
    }
    assert!(openai.requests().await.is_empty());
    assert_eq!(grok.requests().await.len(), if cold { 1 } else { 3 });
    // The process default is OpenAI; a normal resume must recover the saved Grok binding.
    let id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.clone(),
            config: Some(
                [(
                    "model_catalog_json".into(),
                    json!(home.path().join("grok-models.json")),
                )]
                .into(),
            ),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse = timeout(WAIT, app.read_response(id)).await??;
    assert_eq!(resumed.model_provider, GROK);
    assert_eq!(resumed.thread.model_provider, GROK);
    assert_eq!(resumed.model, GROK_MODEL);
    let turn = begin(&mut app, &thread, "resume using the same provider").await?;
    completed(&mut app, &thread, &turn, TurnStatus::Completed).await?;
    assert_eq!(
        assistant_texts(&mut app, &thread).await?,
        if cold {
            vec!["GROK_SEED", "GROK_RESUMED"]
        } else {
            vec![
                "GROK_SEED",
                "GROK_AFTER_CONFIG_OVERRIDE",
                "GROK_AFTER_TYPED_OVERRIDE",
                "GROK_RESUMED",
            ]
        }
    );
    let bodies = request_bodies(&grok).await?;
    assert!(
        bodies[1]["input"]
            .as_array()
            .context("resume input array")?
            .iter()
            .any(|item| { item["type"] == "reasoning" && item["encrypted_content"] == CIPHERTEXT })
    );
    assert!(openai.requests().await.is_empty());
    assert_route(&grok, GROK, if cold { 2 } else { 4 }).await?;
    app.shutdown_gracefully().await?;
    openai.shutdown().await;
    grok.shutdown().await;
    Ok(())
}

#[test_case::test_case(false, true; "loaded_forbidden")]
#[test_case::test_case(true, false; "cold_missing")]
#[test_case::test_case(true, true; "cold_forbidden")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_runtime_unavailable_provider_fails_closed_and_recovers(
    cold: bool,
    forbidden: bool,
) -> Result<()> {
    let (openai, _) = start_streaming_sse_server(vec![]).await;
    let (grok, _) = start_streaming_sse_server(vec![
        reply("seed", "GROK_SEED"),
        reply("recovered", "GROK_RECOVERED"),
    ])
    .await;
    let home = TempDir::new()?;
    write_fixture(home.path(), openai.uri(), grok.uri())?;
    let mut app = app(home.path()).await?;
    let thread = start(&mut app, home.path(), GROK).await?;
    let turn = begin(&mut app, &thread, "seed").await?;
    completed(&mut app, &thread, &turn, TurnStatus::Completed).await?;
    if cold {
        app.shutdown_gracefully().await?;
    }
    let config_path = home.path().join("config.toml");
    let original = std::fs::read_to_string(&config_path)?;
    let expected = if forbidden {
        std::fs::write(
            home.path().join("requirements.toml"),
            "model_provider = 'openai'\n",
        )?;
        ModelProviderError::ModelProviderPolicyConflict {
            model_provider: GROK.into(),
            required_model_provider: OPENAI.into(),
        }
    } else {
        let mut config: toml::Value = toml::from_str(&original)?;
        config["model_providers"]
            .as_table_mut()
            .context("provider map")?
            .remove(GROK);
        std::fs::write(&config_path, toml::to_string(&config)?)?;
        ModelProviderError::ModelProviderNotConfigured {
            model_provider: GROK.into(),
        }
    };
    if cold {
        app = self::app(home.path()).await?;
    }
    let id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.clone(),
            ..Default::default()
        })
        .await?;
    expect_provider_error(&mut app, id, &expected).await?;
    if !cold {
        let id = app
            .send_turn_start_request(TurnStartParams {
                thread_id: thread.clone(),
                input: vec![UserInput::Text {
                    text: "MUST_NOT_BE_ADMITTED".into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        expect_provider_error(&mut app, id, &expected).await?;
    }
    assert!(openai.requests().await.is_empty());
    assert_eq!(grok.requests().await.len(), 1);
    std::fs::write(config_path, original)?;
    if forbidden {
        std::fs::write(home.path().join("requirements.toml"), "")?;
    }
    let id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.clone(),
            config: Some(
                [(
                    "model_catalog_json".into(),
                    json!(home.path().join("grok-models.json")),
                )]
                .into(),
            ),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse = timeout(WAIT, app.read_response(id)).await??;
    assert_eq!(resumed.model_provider, GROK);
    let turn = begin(&mut app, &thread, "recover with original provider").await?;
    completed(&mut app, &thread, &turn, TurnStatus::Completed).await?;
    assert_eq!(
        assistant_texts(&mut app, &thread).await?,
        vec!["GROK_SEED", "GROK_RECOVERED"]
    );
    assert_route(&grok, GROK, 2).await?;
    assert!(
        request_bodies(&grok)
            .await?
            .iter()
            .all(|body| !body.to_string().contains("MUST_NOT_BE_ADMITTED"))
    );
    assert!(openai.requests().await.is_empty());
    app.shutdown_gracefully().await?;
    openai.shutdown().await;
    grok.shutdown().await;
    Ok(())
}
