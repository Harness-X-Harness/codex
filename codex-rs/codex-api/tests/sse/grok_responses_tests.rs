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
            {"added":{"type":"reasoning","id":"r1","summary":[],"encrypted_content":null}},
            {"summary":"R","index":0},
            {"done":{"type":"reasoning","id":"r1","summary":[],"encrypted_content":null}},
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
    let indexes = (1..=65).map(added).collect();
    let events = (0..4097)
        .map(|_| json!({"type":"response.output_text.delta","output_index":1,"delta":"x"}))
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
