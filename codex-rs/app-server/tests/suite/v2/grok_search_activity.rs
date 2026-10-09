//! Schematic deterministic wire fixtures, not backend captures. The public
//! client must observe start AND settlement before message@0 can finish.
use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::routing::post;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::SearchActivityKind;
use codex_app_server_protocol::SearchActivityNotification;
use codex_app_server_protocol::SearchActivityState;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::time::timeout;

#[cfg(any(target_os = "macos", windows))]
const WAIT: Duration = Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const WAIT: Duration = Duration::from_secs(10);

struct GatedSearchServer {
    uri: String,
    frames: Vec<mpsc::Sender<Option<String>>>,
    requests: mpsc::Receiver<Value>,
    task: tokio::task::JoinHandle<()>,
}

impl GatedSearchServer {
    async fn start() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let uri = format!("http://{}", listener.local_addr()?);
        let (request_tx, requests) = mpsc::channel(4);
        let mut frames = Vec::new();
        let mut receivers = VecDeque::new();
        for _ in 0..3 {
            let (tx, rx) = mpsc::channel::<Option<String>>(8);
            frames.push(tx);
            receivers.push_back(rx);
        }
        let receivers = Arc::new(Mutex::new(receivers));
        let app = Router::new().route(
            "/api/codex/responses",
            post(move |Json(body): Json<Value>| {
                let request_tx = request_tx.clone();
                let receivers = Arc::clone(&receivers);
                async move {
                    request_tx.send(body).await.expect("request capture");
                    let rx = receivers
                        .lock()
                        .await
                        .pop_front()
                        .expect("scripted request");
                    let body =
                        Body::from_stream(futures::stream::unfold(rx, |mut rx| async move {
                            let frame = rx.recv().await.flatten()?;
                            Some((Ok::<_, Infallible>(frame), rx))
                        }));
                    ([("content-type", "text/event-stream")], body)
                }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("gated model server");
        });
        Ok(Self {
            uri,
            frames,
            requests,
            task,
        })
    }

    async fn send(&self, request: usize, events: Vec<Value>) -> Result<()> {
        self.frames[request]
            .send(Some(responses::sse(events)))
            .await?;
        Ok(())
    }
}

impl Drop for GatedSearchServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Fixture {
    home: TempDir,
    server: GatedSearchServer,
    app: TestAppServer,
    thread_id: String,
    turn_id: String,
}

impl Fixture {
    async fn start(retries: u32, idle_timeout_ms: u64) -> Result<Self> {
        let server = GatedSearchServer::start().await?;
        let home = TempDir::new()?;
        super::grok_provider_binding::write_grok_fixture(home.path(), &server.uri)?;
        let path = home.path().join("config.toml");
        let mut config: toml::Value = toml::from_str(&std::fs::read_to_string(&path)?)?;
        let root = config.as_table_mut().context("fixture config table")?;
        root.insert("web_search".into(), "live".into());
        let update_plan = root
            .get_mut("tools")
            .and_then(toml::Value::as_table_mut)
            .and_then(|tools| tools.get_mut("update_plan"))
            .and_then(toml::Value::as_table_mut)
            .context("fixture update_plan tool table")?;
        update_plan.insert("enabled".into(), true.into());
        let provider = root
            .get_mut("model_providers")
            .and_then(toml::Value::as_table_mut)
            .and_then(|providers| providers.get_mut("grok"))
            .and_then(toml::Value::as_table_mut)
            .context("fixture Grok provider table")?;
        provider.insert("stream_max_retries".into(), i64::from(retries).into());
        provider.insert(
            "stream_idle_timeout_ms".into(),
            i64::try_from(idle_timeout_ms)?.into(),
        );
        std::fs::write(path, toml::to_string(&config)?)?;
        let mut app = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized_with_timeout(WAIT)
            .await?;
        let request = app
            .send_thread_start_request_with_auto_env(ThreadStartParams {
                model: Some("grok-4.6".into()),
                ..Default::default()
            })
            .await?;
        let started: ThreadStartResponse = timeout(WAIT, app.read_response(request)).await??;
        let thread_id = started.thread.id;
        let mut fixture = Self {
            home,
            server,
            app,
            thread_id,
            turn_id: String::new(),
        };
        fixture.start_turn().await?;
        Ok(fixture)
    }

    async fn start_turn(&mut self) -> Result<()> {
        let request = self
            .app
            .send_turn_start_request(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: "Search fixture query".into(),
                    text_elements: vec![],
                }],
                ..Default::default()
            })
            .await?;
        let started: TurnStartResponse = timeout(WAIT, self.app.read_response(request)).await??;
        self.turn_id = started.turn.id;
        Ok(())
    }
}

fn web(status: &str) -> Value {
    json!({"type":"web_search_call", "id":"web-1", "status":status,
        "action":{"type":"search", "query":"fixture query"}})
}

fn prefix() -> Vec<Value> {
    let mut added = responses::ev_message_item_added("message-0", "");
    added["output_index"] = json!(0);
    vec![
        responses::ev_response_created("response-reused"),
        added,
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"message-0","delta":"A"}),
        json!({"type":"response.output_item.added","output_index":1,"item":web("in_progress")}),
        json!({"type":"response.web_search_call.searching","output_index":1,"item_id":"web-1"}),
    ]
}

fn settlement() -> Vec<Value> {
    vec![
        json!({"type":"response.web_search_call.completed","output_index":1,"item_id":"web-1"}),
        json!({"type":"response.output_item.done","output_index":1,"item":web("completed")}),
    ]
}

fn suffix() -> Vec<Value> {
    let mut done = responses::ev_assistant_message("message-0", "AB");
    done["output_index"] = json!(0);
    vec![
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"message-0","delta":"B"}),
        done,
        responses::ev_completed("response-reused"),
    ]
}

#[derive(Default)]
struct Observed {
    text: String,
    canonical_searches: Vec<Value>,
    activity: Vec<SearchActivityNotification>,
}

impl Observed {
    async fn until(&mut self, app: &mut TestAppServer, method: &str) -> Result<Value> {
        timeout(WAIT, async {
            loop {
                let JSONRPCMessage::Notification(notification) = app.read_next_message().await?
                else {
                    continue;
                };
                let params = notification.params.context("notification params")?;
                match notification.method.as_str() {
                    "item/agentMessage/delta" => self
                        .text
                        .push_str(params["delta"].as_str().context("text delta")?),
                    "item/searchActivity" => {
                        self.activity.push(serde_json::from_value(params.clone())?)
                    }
                    "item/completed" if params["item"]["type"] == "webSearch" => {
                        self.canonical_searches.push(params["item"].clone())
                    }
                    _ => {}
                }
                if notification.method == method {
                    return Ok(params);
                }
            }
        })
        .await?
    }
}

#[tokio::test]
async fn grok_public_search_activity_gates_start_settlement_and_exact_transcript() -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let first_request = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("first request")?;
    assert!(
        first_request["tools"]
            .as_array()
            .context("tools")?
            .iter()
            .any(|tool| tool["type"] == "web_search")
    );
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let running = seen.activity.last().context("running activity")?.clone();
    assert_eq!(
        (
            &running.thread_id,
            &running.turn_id,
            running.output_index,
            running.item_id.as_str(),
            running.kind,
            running.state
        ),
        (
            &fixture.thread_id,
            &fixture.turn_id,
            1,
            "web-1",
            SearchActivityKind::Web,
            SearchActivityState::Running
        )
    );
    assert_eq!(seen.text, "A");
    assert!(seen.canonical_searches.is_empty());
    fixture.server.send(/*request*/ 0, settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let expected_settlement = SearchActivityNotification {
        state: SearchActivityState::Completed,
        ..running.clone()
    };
    assert_eq!(seen.activity, vec![running, expected_settlement]);
    assert_eq!(seen.text, "A");
    assert!(seen.canonical_searches.is_empty());
    // Gate opens only after BOTH public observations. No timing sleep can satisfy it.
    fixture.server.send(/*request*/ 0, suffix()).await?;
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    assert_eq!(seen.text, "AB");
    assert_eq!(seen.canonical_searches.len(), 1);
    assert_eq!(seen.activity.len(), 2);
    fixture.start_turn().await?;
    let followup = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("followup request")?;
    let hosted = followup["input"]
        .as_array()
        .context("followup input")?
        .iter()
        .filter(|item| item["type"] == "web_search_call")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        hosted,
        vec![
            json!({"type":"web_search_call","id":"web-1","action":{"type":"search","query":"fixture query"}})
        ]
    );
    assert!(
        followup["input"]
            .as_array()
            .context("follow-up input array")?
            .iter()
            .all(|item| item["call_id"] != "web-1")
    );
    let mut reply = responses::ev_message_item_added("followup", "");
    reply["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message("followup", "done");
    done["output_index"] = json!(0);
    fixture
        .server
        .send(
            /*request*/ 1,
            vec![reply, done, responses::ev_completed("followup")],
        )
        .await?;
    seen.until(&mut fixture.app, "turn/completed").await?;
    drop(fixture.app);
    let mut cold = TestAppServer::builder()
        .with_codex_home(fixture.home.path())
        .build_initialized_with_timeout(WAIT)
        .await?;
    let request = cold
        .send_thread_read_request(ThreadReadParams {
            thread_id: fixture.thread_id,
            include_turns: true,
        })
        .await?;
    let read: ThreadReadResponse = timeout(WAIT, cold.read_response(request)).await??;
    let items = read
        .thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter(|item| matches!(item, ThreadItem::WebSearch(_)))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 1);
    assert_eq!(serde_json::to_value(items[0])?, seen.canonical_searches[0]);
    assert!(
        !cold
            .pending_notification_methods()
            .iter()
            .any(|method| method == "item/searchActivity")
    );
    Ok(())
}

#[test_case("eof"; "eof")]
#[test_case("failed"; "failed")]
#[test_case("incomplete"; "incomplete")]
#[test_case("malformed"; "malformed")]
#[test_case("cancel"; "cancel")]
#[test_case("timeout"; "timeout")]
#[tokio::test]
async fn grok_public_search_activity_clears_completed_unretained_attempt(
    failure: &str,
) -> Result<()> {
    let idle = if failure == "timeout" { 2_000 } else { 120_000 };
    let mut fixture = Fixture::start(/*retries*/ 0, idle).await?;
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.send(/*request*/ 0, settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    match failure {
        "eof" => fixture.server.frames[0].send(None).await?,
        "failed" => fixture.server.send(/*request*/ 0, vec![json!({"type":"response.failed","response":{"error":{"code":"server_error","message":"fixture failure"}}})]).await?,
        "incomplete" => fixture.server.send(/*request*/ 0, vec![json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}})]).await?,
        "malformed" => fixture.server.frames[0].send(Some("data: malformed-json\n\n".into())).await?,
        "cancel" => { fixture.app.send_turn_interrupt_request(TurnInterruptParams { thread_id: fixture.thread_id.clone(), turn_id: fixture.turn_id.clone() }).await?; },
        "timeout" => {},
        _ => unreachable!(),
    }
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let expected = SearchActivityNotification {
        state: SearchActivityState::Cleared,
        ..seen.activity[0].clone()
    };
    assert_eq!(seen.activity.last(), Some(&expected));
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert!(matches!(
        completed.turn.status,
        TurnStatus::Failed | TurnStatus::Interrupted
    ));
    assert!(seen.canonical_searches.is_empty());
    assert!(
        !completed
            .turn
            .items
            .iter()
            .any(|item| matches!(item, ThreadItem::WebSearch(_)))
    );
    Ok(())
}

#[tokio::test]
async fn grok_public_search_activity_retry_reuses_provider_ids_in_a_new_attempt() -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 1, /*idle_timeout_ms*/ 120_000).await?;
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.send(/*request*/ 0, settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.frames[0].send(None).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    assert_eq!(seen.activity[2].state, SearchActivityState::Cleared);
    let old_attempt = seen.activity[0].attempt_id;
    fixture.server.send(/*request*/ 1, prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    assert!(seen.activity[3].attempt_id > old_attempt);
    assert_eq!(seen.activity[3].item_id, seen.activity[0].item_id);
    fixture.server.send(/*request*/ 1, settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.send(/*request*/ 1, suffix()).await?;
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    assert_eq!(seen.canonical_searches.len(), 1);
    assert_eq!(
        seen.activity.iter().map(|a| a.state).collect::<Vec<_>>(),
        vec![
            SearchActivityState::Running,
            SearchActivityState::Completed,
            SearchActivityState::Cleared,
            SearchActivityState::Running,
            SearchActivityState::Completed,
        ]
    );
    Ok(())
}

fn indexed_web(index: u64, query: &str) -> Vec<Value> {
    let mut running = web("in_progress");
    running["action"]["query"] = json!(query);
    let mut completed = running.clone();
    completed["status"] = json!("completed");
    vec![
        json!({"type":"response.output_item.added","output_index":index,"item":running}),
        json!({"type":"response.output_item.done","output_index":index,"item":completed}),
    ]
}

async fn read_thread(app: &mut TestAppServer, thread_id: &str) -> Result<ThreadReadResponse> {
    let request = app
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.into(),
            include_turns: true,
        })
        .await?;
    timeout(WAIT, app.read_response(request)).await?
}

fn assert_first_search_preserved(
    read: &ThreadReadResponse,
    turn_id: &str,
    expected: &Value,
) -> Result<()> {
    let turn = read
        .thread
        .turns
        .iter()
        .find(|turn| turn.id == turn_id)
        .context("original turn")?;
    let same_id = turn
        .items
        .iter()
        .map(serde_json::to_value)
        .collect::<serde_json::Result<Vec<_>>>()?
        .into_iter()
        .filter(|item| item["id"] == "web-1")
        .collect::<Vec<_>>();
    assert_eq!(same_id, vec![expected.clone()]);
    assert_eq!(expected["query"], "first");
    Ok(())
}

#[derive(Clone, Copy)]
enum SearchCollision {
    SameResponse,
    NextSampling,
    LaterMessage,
    LaterLocalCall,
}

#[test_case(SearchCollision::SameResponse; "same_response")]
#[test_case(SearchCollision::NextSampling; "next_sampling_same_turn")]
#[test_case(SearchCollision::LaterMessage; "later_non_web_item")]
#[test_case(SearchCollision::LaterLocalCall; "later_local_call_identity")]
#[tokio::test]
async fn grok_public_search_activity_retained_id_cannot_be_rebound(
    collision: SearchCollision,
) -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let original_turn_id = fixture.turn_id.clone();
    let request = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("first request")?;
    let mut seen = Observed::default();
    fixture
        .server
        .send(/*request*/ 0, indexed_web(/*index*/ 0, "first"))
        .await?;
    while seen.canonical_searches.is_empty() {
        seen.until(&mut fixture.app, "item/completed").await?;
    }
    assert_eq!(seen.activity.len(), 2);
    let first_canonical = seen.canonical_searches[0].clone();
    let (stream, index) = if matches!(collision, SearchCollision::NextSampling) {
        fixture
            .server
            .send(
                /*request*/ 0,
                vec![json!({"type":"response.completed",
            "response":{"id":"first-sampling","end_turn":false}})],
            )
            .await?;
        let followup = timeout(WAIT, fixture.server.requests.recv())
            .await?
            .context("same-turn followup")?;
        assert_eq!(
            followup["input"]
                .as_array()
                .context("same-turn follow-up input array")?
                .iter()
                .filter(|item| item["type"] == "web_search_call")
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                json!({"type":"web_search_call","id":"web-1","action":{"type":"search","query":"first"}})
            ]
        );
        (1, 0)
    } else {
        (0, 1)
    };
    let frames = match collision {
        SearchCollision::SameResponse | SearchCollision::NextSampling => {
            indexed_web(index, "second")
        }
        SearchCollision::LaterMessage => {
            let item = json!({"type":"message","id":"web-1","role":"assistant",
                "content":[{"type":"output_text","text":"second"}]});
            vec![
                json!({"type":"response.output_item.added","output_index":index,"item":item}),
                json!({"type":"response.output_item.done","output_index":index,"item":item}),
            ]
        }
        SearchCollision::LaterLocalCall => {
            let name = codex_tools::flat_wire_name(
                "function",
                &codex_tools::ToolName::plain("update_plan"),
            );
            assert!(
                request["tools"]
                    .as_array()
                    .context("advertised tool array")?
                    .iter()
                    .any(|tool| tool["name"] == name)
            );
            let item = json!({"type":"function_call","id":"local-raw","call_id":"web-1","name":name,
                "arguments":json!({"plan":[{"step":"must not replace retained search","status":"in_progress"}]}).to_string()});
            vec![
                json!({"type":"response.output_item.added","output_index":index,"item":item}),
                json!({"type":"response.output_item.done","output_index":index,"item":item}),
            ]
        }
    };
    fixture.server.send(stream, frames).await?;
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(completed.thread_id, fixture.thread_id);
    assert_eq!(completed.turn.id, original_turn_id);
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    // Failed-turn notifications omit items; read/replay below proves retention.
    assert_eq!(completed.turn.items_view, TurnItemsView::NotLoaded);
    assert_eq!(completed.turn.items, Vec::<ThreadItem>::new());
    let reason = match collision {
        SearchCollision::SameResponse | SearchCollision::NextSampling => {
            "search item ID is already used in this turn"
        }
        SearchCollision::LaterMessage | SearchCollision::LaterLocalCall => {
            "retained search item ID cannot be rebound in this turn"
        }
    };
    assert_eq!(
        completed.turn.error,
        Some(TurnError {
            misalignment: None,
            message: format!("stream disconnected before completion: {reason}"),
            codex_error_info: Some(CodexErrorInfo::Other),
            additional_details: None,
        })
    );

    assert_eq!(
        seen.activity
            .iter()
            .map(|activity| activity.state)
            .collect::<Vec<_>>(),
        vec![SearchActivityState::Running, SearchActivityState::Completed]
    );
    assert_eq!(seen.canonical_searches, vec![first_canonical.clone()]);
    let warm = read_thread(&mut fixture.app, &fixture.thread_id).await?;
    assert_first_search_preserved(&warm, &original_turn_id, &first_canonical)?;

    // A real subsequent request must replay only the first retained result.
    fixture.start_turn().await?;
    let followup = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("post-failure followup")?;
    assert_eq!(
        followup["input"]
            .as_array()
            .context("follow-up input array")?
            .iter()
            .filter(|item| item["type"] == "web_search_call")
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            json!({"type":"web_search_call","id":"web-1","action":{"type":"search","query":"first"}})
        ]
    );
    let mut added = responses::ev_message_item_added("followup", "");
    added["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message("followup", "done");
    done["output_index"] = json!(0);
    fixture
        .server
        .send(
            stream + 1,
            vec![added, done, responses::ev_completed("followup")],
        )
        .await?;
    seen.until(&mut fixture.app, "turn/completed").await?;
    drop(fixture.app);
    let mut cold = TestAppServer::builder()
        .with_codex_home(fixture.home.path())
        .build_initialized_with_timeout(WAIT)
        .await?;
    let read = read_thread(&mut cold, &fixture.thread_id).await?;
    assert_first_search_preserved(&read, &original_turn_id, &first_canonical)?;
    Ok(())
}

#[tokio::test]
async fn grok_public_search_activity_new_turn_may_reuse_a_retained_provider_id() -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let mut seen = Observed::default();
    let mut turns = Vec::new();
    for (request, query) in ["first", "second"].into_iter().enumerate() {
        if request > 0 {
            fixture.start_turn().await?;
        }
        turns.push(fixture.turn_id.clone());
        let mut frames = indexed_web(/*index*/ 0, query);
        frames.push(responses::ev_completed(query));
        fixture.server.send(request, frames).await?;
        let completed: TurnCompletedNotification =
            serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
        assert_eq!(completed.turn.status, TurnStatus::Completed);
    }
    assert_eq!(seen.activity.len(), 4);
    assert_eq!(
        seen.canonical_searches
            .iter()
            .map(|item| item["query"].as_str().context("canonical search query"))
            .collect::<Result<Vec<_>>>()?,
        vec!["first", "second"]
    );
    let read = read_thread(&mut fixture.app, &fixture.thread_id).await?;
    assert_first_search_preserved(&read, &turns[0], &seen.canonical_searches[0])?;
    assert_eq!(read.thread.turns.iter().find(|turn| turn.id == turns[1]).context("second turn retained history")?.items.iter()
        .filter(|item| matches!(item, ThreadItem::WebSearch(search) if search.id == "web-1" && search.query == "second")).count(), 1);
    Ok(())
}

#[tokio::test]
async fn grok_public_search_activity_multiple_interleaved_searches_keep_their_identity()
-> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let mut second = web("in_progress");
    second["id"] = json!("web-2");
    fixture
        .server
        .send(
            /*request*/ 0,
            vec![json!({"type":"response.output_item.added","output_index":2,"item":second})],
        )
        .await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    second["status"] = json!("completed");
    fixture
        .server
        .send(
            /*request*/ 0,
            vec![json!({"type":"response.output_item.done","output_index":2,"item":second})],
        )
        .await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.send(/*request*/ 0, settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    assert_eq!(
        seen.activity
            .iter()
            .map(|activity| (
                activity.output_index,
                activity.item_id.as_str(),
                activity.state
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, "web-1", SearchActivityState::Running),
            (2, "web-2", SearchActivityState::Running),
            (2, "web-2", SearchActivityState::Completed),
            (1, "web-1", SearchActivityState::Completed),
        ]
    );
    assert!(seen.canonical_searches.is_empty());
    fixture.server.send(/*request*/ 0, suffix()).await?;
    seen.until(&mut fixture.app, "turn/completed").await?;
    assert_eq!(seen.text, "AB");
    assert_eq!(
        seen.canonical_searches
            .iter()
            .map(|item| item["id"].as_str().context("canonical search item ID"))
            .collect::<Result<Vec<_>>>()?,
        vec!["web-1", "web-2"]
    );
    Ok(())
}
