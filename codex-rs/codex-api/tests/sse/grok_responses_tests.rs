use super::*;
use codex_api::ApiDialect;
use codex_api::ApiError;
use codex_api::ResponsesOptions;
use pretty_assertions::assert_eq;
use serde_json::json;

type StreamEvents = Vec<Result<ResponseEvent, ApiError>>;

async fn run_body(body: String, dialect: ApiDialect) -> Result<StreamEvents> {
    let client = ResponsesClient::new(
        FixtureSseTransport::new(body),
        provider("grok"),
        Arc::new(NoAuth),
    )
    .with_dialect(dialect);
    let stream = client
        .stream_request(common::basic_request(vec![]), ResponsesOptions::default())
        .await?;
    Ok(stream.collect().await)
}

async fn run_events(events: Vec<Value>, dialect: ApiDialect) -> Result<StreamEvents> {
    run_body(build_responses_body(events), dialect).await
}

fn item_frame(kind: &str, index: u64, item: Value) -> Value {
    json!({"type":kind, "output_index":index, "item":item})
}

fn message() -> Value {
    json!({"type":"message", "id":"m0", "role":"assistant", "content":[]})
}

fn added(index: u64) -> Value {
    item_frame("response.output_item.added", index, message())
}

fn done(index: u64) -> Value {
    item_frame("response.output_item.done", index, message())
}

fn completed() -> Value {
    json!({"type":"response.completed", "response":{"id":"r"}})
}

fn item_trace(events: StreamEvents) -> Result<Vec<Value>> {
    events
        .into_iter()
        .filter(|event| !matches!(event, Ok(ResponseEvent::RateLimits(_))))
        .map(|event| -> Result<Value> {
            Ok(match event? {
                ResponseEvent::OutputItemAdded(item) => json!({"added":item}),
                ResponseEvent::OutputItemDone(item) => json!({"done":item}),
                ResponseEvent::OutputTextDelta(text) => json!({"text":text}),
                ResponseEvent::ReasoningSummaryDelta {
                    delta,
                    summary_index,
                } => json!({"summary":delta,"index":summary_index}),
                ResponseEvent::Completed { response_id, .. } => json!({"completed":response_id}),
                other => anyhow::bail!("unexpected event: {other:?}"),
            })
        })
        .collect()
}

fn assert_failed(events: StreamEvents) {
    assert!(events.last().is_some_and(Result::is_err), "{events:?}");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Ok(ResponseEvent::Completed { .. }))),
        "{events:?}"
    );
}

#[tokio::test]
async fn grok_interleaving_and_later_index_first_arrival_preserve_item_lifetimes() -> Result<()> {
    let reasoning = json!({"type":"reasoning","id":"r1","summary":[],"encrypted_content":null});
    let later_index = 1;
    let events = vec![
        item_frame("response.output_item.added", later_index, reasoning.clone()),
        json!({"type":"response.reasoning_summary_text.delta","output_index":later_index,"summary_index":0,"delta":"R"}),
        added(/*index*/ 0),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"A"}),
        item_frame("response.output_item.done", later_index, reasoning.clone()),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"B"}),
        done(/*index*/ 0),
        json!({"type":"response.completed","response":{"id":"r","output":[message(),reasoning]}}),
    ];
    assert_eq!(
        json!(item_trace(run_events(events, ApiDialect::Grok).await?)?),
        json!([
            {"added":message()}, {"text":"A"}, {"text":"B"}, {"done":message()},
            {"added":{"type":"reasoning","id":"r1","summary":[],"content":null,"encrypted_content":null}},
            {"summary":"R","index":0},
            {"done":{"type":"reasoning","id":"r1","summary":[],"content":null,"encrypted_content":null}},
            {"completed":"r"}
        ])
    );
    Ok(())
}

#[tokio::test]
async fn grok_never_accepts_completion_over_an_open_item_or_index_gap() -> Result<()> {
    let traces = json!([
        [added(/*index*/ 0), completed()],
        [added(/*index*/ 1), done(/*index*/ 1), completed()],
        [added(/*index*/ 0), added(/*index*/ 1), done(/*index*/ 1), completed()],
        [{"type":"response.completed","response":{"id":"r","output":[message()]}}],
        [{"type":"response.completed","output_index":0,"response":{"id":"r"}}]
    ]);
    for events in serde_json::from_value::<Vec<Vec<Value>>>(traces)? {
        assert_failed(run_events(events, ApiDialect::Grok).await?);
    }
    Ok(())
}

#[tokio::test]
async fn grok_rejects_late_missing_malformed_and_out_of_order_item_frames() -> Result<()> {
    let added = added(/*index*/ 0);
    let done = done(/*index*/ 0);
    let traces = json!([
        [added.clone(), done.clone(), {"type":"response.output_text.delta","output_index":0,"delta":"late"}, completed()],
        [added.clone(), added.clone(), completed()], [done, completed()],
        [{"type":"response.output_text.delta","output_index":0,"delta":"early"}, completed()],
        [added, {"type":"response.output_text.delta","delta":"unidentified"}, completed()],
        [item_frame("response.output_item.added", /*index*/ 0, json!({"type":"future_item"})), completed()],
        [item_frame("response.output_item.added", /*index*/ 0, json!({"type":"message"})), completed()]
    ]);
    for events in serde_json::from_value::<Vec<Vec<Value>>>(traces)? {
        assert_failed(run_events(events, ApiDialect::Grok).await?);
    }
    assert_failed(run_body("data: malformed-json\n\n".into(), ApiDialect::Grok).await?);
    Ok(())
}

#[tokio::test]
async fn grok_bounds_pending_indexes_events_and_encoded_bytes() -> Result<()> {
    let indexes = (1..=65)
        .map(|index| {
            let mut item = message();
            item["id"] = json!(format!("m{index}"));
            item_frame("response.output_item.added", index, item)
        })
        .collect();
    let events = std::iter::once(added(/*index*/ 1))
        .chain(
            (0..4096)
                .map(|_| json!({"type":"response.output_text.delta","output_index":1,"delta":"x"})),
        )
        .collect();
    let bytes = vec![
        json!({"type":"response.output_text.delta","output_index":1,"delta":"x".repeat(2 * 1024 * 1024)}),
    ];
    for input in [indexes, events, bytes] {
        let mut result = run_events(input, ApiDialect::Grok).await?;
        assert!(
            matches!(result.pop(), Some(Err(ApiError::Stream(message))) if message == "Grok stream: pending output buffer limit exceeded")
        );
    }
    Ok(())
}

#[tokio::test]
async fn grok_backend_failure_and_transport_close_cannot_become_completion() -> Result<()> {
    let events = vec![
        json!({"type":"response.failed","response":{"error":{"code":"context_length_exceeded","message":"too long"}}}),
        completed(),
    ];
    assert!(matches!(
        run_events(events, ApiDialect::Grok).await?.pop(),
        Some(Err(ApiError::ContextWindowExceeded))
    ));
    let traces = json!([
        [{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}, completed()],
        [{"type":"error","error":{"message":"backend failure"}}, completed()],
        [added(/*index*/ 0)]
    ]);
    for events in serde_json::from_value::<Vec<Vec<Value>>>(traces)? {
        assert_failed(run_events(events, ApiDialect::Grok).await?);
    }
    Ok(())
}

#[tokio::test]
async fn stock_ingress_preserves_arrival_order_and_ignores_unknown_index_shape() -> Result<()> {
    let events = vec![
        added(/*index*/ 1),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"stock"}),
        json!({"type":"response.completed","output_index":"ignored","response":{"id":"r"}}),
    ];
    assert_eq!(
        json!(item_trace(run_events(events, ApiDialect::OpenAi).await?)?),
        json!([
            {"added":message()}, {"text":"stock"},
            {"completed":"r"}
        ])
    );
    let body = "data: malformed-json\n\n".to_owned() + &build_responses_body(vec![completed()]);
    assert_eq!(
        json!(item_trace(run_body(body, ApiDialect::OpenAi).await?)?),
        json!([
            {"completed":"r"}
        ])
    );
    Ok(())
}

fn web_item(status: &str) -> Value {
    json!({"type":"web_search_call", "id":"web-1", "status":status,
        "action":{"type":"search", "query":"fixture query"}})
}

fn web_request() -> Result<codex_api::ResponsesApiRequest> {
    let mut request = common::basic_request(vec![]);
    request.tools = Some(
        Arc::<serde_json::value::RawValue>::from(serde_json::value::to_raw_value(
            &json!([{"type":"web_search"}]),
        )?)
        .into(),
    );
    Ok(request)
}

async fn run_web_events(events: Vec<Value>, dialect: ApiDialect) -> Result<StreamEvents> {
    let client = ResponsesClient::new(
        FixtureSseTransport::new(build_responses_body(events)),
        provider("grok"),
        Arc::new(NoAuth),
    )
    .with_dialect(dialect);
    Ok(client
        .stream_request(web_request()?, ResponsesOptions::default())
        .await?
        .collect()
        .await)
}

#[tokio::test]
async fn grok_search_activity_requires_admitted_request_and_explicit_dialect() -> Result<()> {
    let events = vec![
        item_frame(
            "response.output_item.added",
            /*index*/ 0,
            web_item("in_progress"),
        ),
        item_frame(
            "response.output_item.done",
            /*index*/ 0,
            web_item("completed"),
        ),
        completed(),
    ];
    for result in [
        run_events(events.clone(), ApiDialect::Grok).await?,
        run_web_events(events.clone(), ApiDialect::OpenAi).await?,
    ] {
        assert!(
            result
                .iter()
                .all(|event| !matches!(event, Ok(ResponseEvent::SearchActivity { .. })))
        );
        assert!(matches!(
            result.last(),
            Some(Ok(ResponseEvent::Completed { .. }))
        ));
    }
    let activity = run_web_events(events, ApiDialect::Grok)
        .await?
        .into_iter()
        .filter_map(|event| match event {
            Ok(ResponseEvent::SearchActivity {
                output_index,
                item_id,
                state,
                ..
            }) => Some((output_index, item_id, state)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        activity,
        vec![
            (
                0,
                "web-1".into(),
                codex_protocol::SearchActivityState::Running
            ),
            (
                0,
                "web-1".into(),
                codex_protocol::SearchActivityState::Completed
            ),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn grok_search_activity_rejects_unbound_conflicting_and_false_completion() -> Result<()> {
    let start = item_frame(
        "response.output_item.added",
        /*index*/ 1,
        web_item("in_progress"),
    );
    let status =
        json!({"type":"response.web_search_call.searching", "output_index":1,"item_id":"web-1"});
    let mut wrong_id = web_item("completed");
    wrong_id["id"] = json!("other");
    let mut missing_action = web_item("completed");
    missing_action.as_object_mut().unwrap().remove("action");
    let invalid = vec![
        vec![status.clone()],
        vec![start.clone(), start.clone()],
        vec![
            start.clone(),
            item_frame("response.output_item.done", /*index*/ 1, wrong_id),
        ],
        vec![
            start.clone(),
            item_frame("response.output_item.done", /*index*/ 1, message()),
        ],
        vec![
            start.clone(),
            item_frame(
                "response.output_item.done",
                /*index*/ 1,
                missing_action,
            ),
        ],
        vec![
            start.clone(),
            item_frame(
                "response.output_item.done",
                /*index*/ 1,
                web_item("in_progress"),
            ),
        ],
        vec![
            start.clone(),
            json!({"type":"response.web_search_call.searching", "output_index":1,"item_id":"other"}),
        ],
        vec![
            start.clone(),
            json!({"type":"response.web_search_call.searching", "item_id":"web-1"}),
        ],
        vec![
            start.clone(),
            item_frame(
                "response.output_item.added",
                /*index*/ 2,
                web_item("in_progress"),
            ),
        ],
        vec![item_frame(
            "response.output_item.added",
            /*index*/ 1,
            web_item("completed"),
        )],
        vec![
            start.clone(),
            json!({"type":"response.web_search_call.completed", "output_index":1,"item_id":"web-1"}),
            status.clone(),
        ],
    ];
    for events in invalid {
        let result = run_web_events(events, ApiDialect::Grok).await?;
        assert!(result.iter().all(|event| !matches!(
            event,
            Ok(ResponseEvent::SearchActivity {
                state: codex_protocol::SearchActivityState::Completed,
                ..
            })
        )));
        assert_failed(result);
    }
    // Equivalent statuses do not create starts, and completed status is not success.
    let result = run_web_events(vec![start, status.clone(), status,
        json!({"type":"response.web_search_call.completed", "output_index":1,"item_id":"web-1"})], ApiDialect::Grok).await?;
    assert_eq!(
        result
            .iter()
            .filter(|event| matches!(event, Ok(ResponseEvent::SearchActivity { .. })))
            .count(),
        1
    );
    assert_failed(result);
    Ok(())
}

#[derive(Clone)]
struct GatedSearchTransport(Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<String>>>>);

impl HttpTransport for GatedSearchTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build("execute should not run".into()))
    }

    async fn stream(&self, _: Request) -> Result<StreamResponse, TransportError> {
        let rx = self.0.lock().await.take().expect("one gated request");
        Ok(StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(futures::stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|data| (Ok(Bytes::from(data)), rx))
            })),
        })
    }
}

#[tokio::test]
async fn grok_search_activity_starts_and_settles_before_canonical_head_can_close() -> Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let mut config = provider("grok");
    config.stream_idle_timeout = Duration::from_secs(10);
    let client = ResponsesClient::new(
        GatedSearchTransport(Arc::new(tokio::sync::Mutex::new(Some(rx)))),
        config,
        Arc::new(NoAuth),
    )
    .with_dialect(ApiDialect::Grok);
    let mut stream = client
        .stream_request(web_request()?, ResponsesOptions::default())
        .await?;
    tx.send(build_responses_body(vec![
        added(/*index*/ 0),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"A"}),
        item_frame(
            "response.output_item.added",
            /*index*/ 1,
            web_item("in_progress"),
        ),
    ]))
    .await?;
    let mut text = String::new();
    let start = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.next().await.expect("gated response")? {
                ResponseEvent::OutputTextDelta(delta) => text.push_str(&delta),
                ResponseEvent::SearchActivity {
                    output_index,
                    item_id,
                    state,
                    ..
                } => break Ok::<_, ApiError>((output_index, item_id, state)),
                ResponseEvent::OutputItemDone(_) | ResponseEvent::Completed { .. } => {
                    panic!("closed gate emitted canonical completion")
                }
                _ => {}
            }
        }
    })
    .await??;
    assert_eq!(
        start,
        (
            1,
            "web-1".into(),
            codex_protocol::SearchActivityState::Running
        )
    );
    assert_eq!(text, "A");
    tx.send(build_responses_body(vec![item_frame(
        "response.output_item.done",
        /*index*/ 1,
        web_item("completed"),
    )]))
    .await?;
    let settled = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await?
        .expect("settlement")?;
    assert!(
        matches!(settled, ResponseEvent::SearchActivity { output_index: 1, item_id, state: codex_protocol::SearchActivityState::Completed, .. } if item_id == "web-1")
    );
    // Neither canonical head closure nor B exists on the wire until both observations.
    tx.send(build_responses_body(vec![
        json!({"type":"response.output_text.delta","output_index":0,"delta":"B"}),
        done(/*index*/ 0),
        completed(),
    ]))
    .await?;
    drop(tx);
    let mut web_done = 0;
    while let Some(event) = stream.next().await {
        match event? {
            ResponseEvent::OutputTextDelta(delta) => text.push_str(&delta),
            ResponseEvent::OutputItemDone(ResponseItem::WebSearchCall { .. }) => web_done += 1,
            ResponseEvent::SearchActivity { .. } => panic!("duplicate activity"),
            _ => {}
        }
    }
    assert_eq!((text, web_done), ("AB".into(), 1));
    Ok(())
}

#[path = "grok_x_activity_tests.rs"]
mod x_activity;
