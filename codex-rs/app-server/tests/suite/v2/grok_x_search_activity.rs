//! X and mixed Web/X public HTTP gates. Source-derived wire shapes, never live claims.
use super::*;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use pretty_assertions::assert_eq;
use test_case::test_case;

const X_INPUT: &str = "{ \"query\": \"fixture 日本語\" }\n";

fn x(status: &str) -> Value {
    json!({"type":"custom_tool_call","id":"x-1","call_id":"shared-call","name":"x_keyword_search","status":status,"input":X_INPUT})
}

fn x_prefix() -> Vec<Value> {
    let mut events = prefix();
    events.truncate(3);
    events.extend([
        json!({"type":"response.output_item.added","output_index":1,"item":x("in_progress")}),
        json!({"type":"response.custom_tool_call_input.done","output_index":1,"item_id":"x-1","input":X_INPUT}),
    ]);
    events
}

fn x_settlement() -> Vec<Value> {
    vec![json!({"type":"response.output_item.done","output_index":1,"item":x("completed")})]
}

#[derive(Clone, Copy)]
enum Composition {
    X,
    WebAndX,
}

#[test_case(Composition::X; "x")]
#[test_case(Composition::WebAndX; "mixed_web_x")]
#[tokio::test]
async fn grok_public_x_activity_gates_both_states_and_preserves_cold_resume(
    composition: Composition,
) -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let first = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("initial request")?;
    assert_eq!(
        first["tools"]
            .as_array()
            .context("tools")?
            .iter()
            .filter(|tool| matches!(tool["type"].as_str(), Some("web_search" | "x_search")))
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            json!({"type":"web_search","filters":{"allowed_domains":["example.com"]}}),
            json!({"type":"x_search","from_date":"2026-10-01","to_date":"2026-10-08"})
        ]
    );
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, x_prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let running = seen.activity[0].clone();
    assert_eq!(
        (
            &running.thread_id,
            &running.turn_id,
            running.item_id.as_str(),
            running.output_index,
            running.kind,
            running.state
        ),
        (
            &fixture.thread_id,
            &fixture.turn_id,
            "x-1",
            1,
            SearchActivityKind::X,
            SearchActivityState::Running
        )
    );
    assert_eq!(seen.text, "A");
    assert!(seen.canonical_searches.is_empty());
    let mixed = matches!(composition, Composition::WebAndX);
    if mixed {
        fixture.server.send(/*request*/ 0,vec![json!({"type":"response.output_item.added","output_index":2,"item":web("in_progress")})]).await?;
        seen.until(&mut fixture.app, "item/searchActivity").await?;
        assert_eq!(
            (
                seen.activity[1].kind,
                seen.activity[1].state,
                seen.activity[1].output_index
            ),
            (SearchActivityKind::Web, SearchActivityState::Running, 2)
        );
        assert_eq!(seen.activity[1].attempt_id, running.attempt_id);
    }
    fixture.server.send(/*request*/ 0, x_settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    assert_eq!(
        seen.activity.last(),
        Some(&SearchActivityNotification {
            state: SearchActivityState::Completed,
            ..running
        })
    );
    if mixed {
        fixture.server.send(/*request*/ 0,vec![json!({"type":"response.output_item.done","output_index":2,"item":web("completed")})]).await?;
        seen.until(&mut fixture.app, "item/searchActivity").await?;
        let web_settlement = seen.activity.last().context("Web settlement activity")?;
        assert_eq!(
            (web_settlement.kind, web_settlement.state),
            (SearchActivityKind::Web, SearchActivityState::Completed)
        );
    }
    assert_eq!(seen.text, "A");
    assert!(seen.canonical_searches.is_empty());
    // Neither message completion nor B is sent until every public settlement arrives.
    let reasoning_index = if mixed { 3 } else { 2 };
    let reasoning = json!({"type":"reasoning","id":"opaque","summary":[],"content":[{"type":"reasoning_text","text":"do not replay plaintext"}],"encrypted_content":"opaque-fixture-bytes"});
    fixture.server.send(/*request*/ 0,vec![
        json!({"type":"response.output_item.added","output_index":reasoning_index,"item":reasoning}),
        json!({"type":"response.output_item.done","output_index":reasoning_index,"item":reasoning}),
    ]).await?;
    fixture.server.send(/*request*/ 0, suffix()).await?;
    let complete: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(complete.turn.status, TurnStatus::Completed);
    assert_eq!(seen.text, "AB");
    assert_eq!(seen.canonical_searches.len(), if mixed { 2 } else { 1 });
    assert_eq!(
        seen.canonical_searches[0],
        json!({"type":"xSearch","id":"x-1","callId":"shared-call","name":"x_keyword_search","input":X_INPUT})
    );
    assert_eq!(seen.activity.len(), if mixed { 4 } else { 2 });
    fixture.start_turn().await?;
    let followup = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("followup")?;
    assert_eq!(followup["tools"], first["tools"]);
    assert_retained_input(&followup)?;
    fixture
        .server
        .send(/*request*/ 1, reply("followup"))
        .await?;
    seen.until(&mut fixture.app, "turn/completed").await?;
    drop(fixture.app);
    let mut cold = TestAppServer::builder()
        .with_codex_home(fixture.home.path())
        .build_initialized_with_timeout(WAIT)
        .await?;
    let request = cold
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: fixture.thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse = timeout(WAIT, cold.read_response(request)).await??;
    let x_rows = resumed
        .thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter(|item| matches!(item, ThreadItem::XSearch(_)))
        .collect::<Vec<_>>();
    assert_eq!(x_rows.len(), 1);
    assert_eq!(serde_json::to_value(x_rows[0])?, seen.canonical_searches[0]);
    assert!(
        !cold
            .pending_notification_methods()
            .iter()
            .any(|method| method == "item/searchActivity")
    );
    fixture.app = cold;
    fixture.start_turn().await?;
    let cold_request = timeout(WAIT, fixture.server.requests.recv())
        .await?
        .context("cold followup")?;
    assert_eq!(cold_request["tools"], first["tools"]);
    assert_retained_input(&cold_request)?;
    fixture
        .server
        .send(/*request*/ 2, reply("cold-followup"))
        .await?;
    seen.until(&mut fixture.app, "turn/completed").await?;
    assert_eq!(seen.activity.len(), if mixed { 4 } else { 2 });
    Ok(())
}

fn assert_retained_input(request: &Value) -> Result<()> {
    let items = request["input"]
        .as_array()
        .context("retained input array")?;
    assert_eq!(
        items
            .iter()
            .filter(|item| item["type"] == "custom_tool_call")
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            json!({"type":"custom_tool_call","id":"x-1","call_id":"shared-call","name":"x_keyword_search","input":X_INPUT})
        ]
    );
    assert!(
        items
            .iter()
            .all(|item| item["type"] != "custom_tool_call_output"
                && item["type"] != "function_call_output")
    );
    assert_eq!(
        items
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .cloned()
            .collect::<Vec<_>>(),
        vec![json!({"type":"reasoning","summary":[],"encrypted_content":"opaque-fixture-bytes"})]
    );
    Ok(())
}

fn reply(id: &str) -> Vec<Value> {
    let mut start = responses::ev_message_item_added(id, "");
    start["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message(id, "done");
    done["output_index"] = json!(0);
    vec![start, done, responses::ev_completed(id)]
}

#[test_case("running_eof")]
#[test_case("running_failed")]
#[test_case("running_incomplete")]
#[test_case("running_malformed")]
#[test_case("running_cancel")]
#[test_case("running_timeout")]
#[test_case("eof")]
#[test_case("failed")]
#[test_case("incomplete")]
#[test_case("malformed")]
#[test_case("cancel")]
#[test_case("timeout")]
#[tokio::test]
async fn grok_public_x_activity_clears_unretained_completion(failure: &str) -> Result<()> {
    let settled = !failure.starts_with("running_");
    let failure = failure.strip_prefix("running_").unwrap_or(failure);
    let idle = if failure == "timeout" { 2_000 } else { 120_000 };
    let mut fixture = Fixture::start(/*retries*/ 0, idle).await?;
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, x_prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    if settled {
        fixture.server.send(/*request*/ 0, x_settlement()).await?;
        seen.until(&mut fixture.app, "item/searchActivity").await?;
    }
    match failure {
        "eof"=>fixture.server.frames[0].send(None).await?,
        "failed"=>fixture.server.send(/*request*/ 0,vec![json!({"type":"response.failed","response":{"error":{"code":"server_error","message":"fixture"}}})]).await?,
        "incomplete"=>fixture.server.send(/*request*/ 0,vec![json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}})]).await?,
        "malformed"=>fixture.server.frames[0].send(Some("data: malformed-json\n\n".into())).await?,
        "cancel"=>{fixture.app.send_turn_interrupt_request(TurnInterruptParams {thread_id:fixture.thread_id.clone(),turn_id:fixture.turn_id.clone()}).await?;},
        "timeout"=>{},
        _=>unreachable!(),
    }
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    assert_eq!(
        seen.activity.last(),
        Some(&SearchActivityNotification {
            state: SearchActivityState::Cleared,
            ..seen.activity[0].clone()
        })
    );
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert!(matches!(
        completed.turn.status,
        TurnStatus::Failed | TurnStatus::Interrupted
    ));
    assert_eq!(
        seen.activity
            .iter()
            .map(|activity| activity.state)
            .collect::<Vec<_>>(),
        if settled {
            vec![
                SearchActivityState::Running,
                SearchActivityState::Completed,
                SearchActivityState::Cleared,
            ]
        } else {
            vec![SearchActivityState::Running, SearchActivityState::Cleared]
        }
    );
    assert!(seen.canonical_searches.is_empty());
    assert!(
        !completed
            .turn
            .items
            .iter()
            .any(|item| matches!(item, ThreadItem::XSearch(_)))
    );
    Ok(())
}

#[tokio::test]
async fn grok_public_x_activity_retry_reuses_only_unretained_identity() -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 1, /*idle_timeout_ms*/ 120_000).await?;
    let mut seen = Observed::default();
    for request in 0..2 {
        fixture.server.send(request, x_prefix()).await?;
        seen.until(&mut fixture.app, "item/searchActivity").await?;
        fixture.server.send(request, x_settlement()).await?;
        seen.until(&mut fixture.app, "item/searchActivity").await?;
        if request == 0 {
            fixture.server.frames[0].send(None).await?;
            seen.until(&mut fixture.app, "item/searchActivity").await?;
            assert_eq!(seen.activity[2].state, SearchActivityState::Cleared);
        }
    }
    assert!(seen.activity[3].attempt_id > seen.activity[0].attempt_id);
    fixture.server.send(/*request*/ 1, suffix()).await?;
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    assert_eq!(seen.canonical_searches.len(), 1);
    Ok(())
}

#[test_case("x")]
#[test_case("web")]
#[test_case("message")]
#[tokio::test]
async fn grok_public_x_activity_rejects_retained_id_rebinding_without_erasing_history(
    next_kind: &str,
) -> Result<()> {
    let mut fixture = Fixture::start(/*retries*/ 0, /*idle_timeout_ms*/ 120_000).await?;
    let mut seen = Observed::default();
    fixture.server.send(/*request*/ 0, x_prefix()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    fixture.server.send(/*request*/ 0, x_settlement()).await?;
    seen.until(&mut fixture.app, "item/searchActivity").await?;
    let mut close_head = suffix();
    close_head.pop();
    fixture.server.send(/*request*/ 0, close_head).await?;
    while seen.canonical_searches.is_empty() {
        seen.until(&mut fixture.app, "item/completed").await?;
    }
    let original = seen.canonical_searches[0].clone();
    let next = match next_kind {
        "x" => x("in_progress"),
        "web" => {
            let mut item = web("in_progress");
            item["id"] = json!("x-1");
            item
        }
        "message" => json!({"type":"message","id":"x-1","role":"assistant","content":[]}),
        _ => unreachable!(),
    };
    fixture
        .server
        .send(
            /*request*/ 0,
            vec![json!({"type":"response.output_item.added","output_index":2,"item":next})],
        )
        .await?;
    let completed: TurnCompletedNotification =
        serde_json::from_value(seen.until(&mut fixture.app, "turn/completed").await?)?;
    assert_eq!(completed.thread_id, fixture.thread_id);
    assert_eq!(completed.turn.id, fixture.turn_id);
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    // Failed-turn notifications omit items; read/replay below proves retention.
    assert_eq!(completed.turn.items_view, TurnItemsView::NotLoaded);
    assert_eq!(completed.turn.items, Vec::<ThreadItem>::new());
    let reason = match next_kind {
        "web" => "search item ID is already used in this turn",
        "x" | "message" => "retained search item ID cannot be rebound in this turn",
        _ => unreachable!(),
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
    assert_eq!(seen.canonical_searches, vec![original.clone()]);
    assert_eq!(seen.activity.len(), 2);
    let warm = read_thread(&mut fixture.app, &fixture.thread_id).await?;
    assert_eq!(warm.thread.id, fixture.thread_id);
    let retained_turn = warm
        .thread
        .turns
        .iter()
        .find(|turn| turn.id == fixture.turn_id)
        .context("original X turn")?;
    let same_id = retained_turn
        .items
        .iter()
        .map(serde_json::to_value)
        .collect::<serde_json::Result<Vec<_>>>()?
        .into_iter()
        .filter(|item| item["id"] == "x-1")
        .collect::<Vec<_>>();
    assert_eq!(same_id, vec![original.clone()]);
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
    let x_rows = read
        .thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter(|item| matches!(item, ThreadItem::XSearch(_)))
        .collect::<Vec<_>>();
    assert_eq!(x_rows.len(), 1);
    assert_eq!(serde_json::to_value(x_rows[0])?, original);
    Ok(())
}
