use super::process_sse_with_treatment;
use crate::common::ResponseEvent;
use crate::common::SafetyBufferingTreatment;
use crate::error::ApiError;
use crate::provider::ApiDialect;
use codex_client::TransportError;
use codex_protocol::models::ResponseItem;
use futures::TryStreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;

#[derive(Debug, PartialEq, Eq)]
enum BoundaryKind {
    Created,
    Added(&'static str),
    Done(&'static str),
    Delta,
    Completed,
}

fn response_item_type(item: &ResponseItem) -> &'static str {
    match item {
        ResponseItem::Reasoning { .. } => "reasoning",
        ResponseItem::Message { .. } => "message",
        ResponseItem::WebSearchCall { .. } => "web_search_call",
        other => panic!("unexpected item in fixture: {other:?}"),
    }
}

fn boundary_kind(event: &ResponseEvent) -> BoundaryKind {
    match event {
        ResponseEvent::Created { .. } => BoundaryKind::Created,
        ResponseEvent::OutputItemAdded(item) => BoundaryKind::Added(response_item_type(item)),
        ResponseEvent::OutputItemDone(item) => BoundaryKind::Done(response_item_type(item)),
        ResponseEvent::OutputTextDelta(_) => BoundaryKind::Delta,
        ResponseEvent::Completed { .. } => BoundaryKind::Completed,
        other => panic!("unexpected event in fixture: {other:?}"),
    }
}

fn item_event(kind: &str, index: u64, item: serde_json::Value) -> serde_json::Value {
    json!({"type": kind, "output_index": index, "item": item})
}

fn text_delta(delta: &str) -> serde_json::Value {
    json!({"type": "response.output_text.delta", "output_index": 1, "delta": delta})
}

fn reasoning_item(id: &str, blob: Option<&str>) -> serde_json::Value {
    match blob {
        Some(blob) => {
            json!({"type": "reasoning", "id": id, "summary": [], "encrypted_content": blob})
        }
        None => json!({"type": "reasoning", "id": id, "summary": []}),
    }
}

fn message_item(content: serde_json::Value) -> serde_json::Value {
    json!({"type": "message", "id": "msg_1", "role": "assistant", "content": content})
}

fn web_search_item(id: &str, action: serde_json::Value) -> serde_json::Value {
    json!({"type": "web_search_call", "id": id, "status": "completed", "action": action})
}

fn interleaved_hosted_sse_events() -> Vec<serde_json::Value> {
    let added = "response.output_item.added";
    let done = "response.output_item.done";
    vec![
        json!({"type": "response.created", "response": {"id": "resp-1"}}),
        item_event(added, 0, reasoning_item("rs_1", None)),
        item_event(done, 0, reasoning_item("rs_1", Some("enc-0"))),
        item_event(added, 1, message_item(json!([]))),
        text_delta("A"),
        text_delta("B"),
        item_event(
            added,
            2,
            web_search_item(
                "ws_1-0",
                json!({"type": "search", "query": "q", "sources": []}),
            ),
        ),
        item_event(
            done,
            2,
            web_search_item("ws_1-0", json!({"type": "search", "query": "q"})),
        ),
        item_event(
            added,
            3,
            web_search_item(
                "ws_1-1",
                json!({"type": "open_page", "url": "https://example.com/"}),
            ),
        ),
        item_event(
            done,
            3,
            web_search_item(
                "ws_1-1",
                json!({"type": "open_page", "url": "https://example.com/"}),
            ),
        ),
        item_event(added, 4, reasoning_item("tco_1-0", Some("enc-tco-0"))),
        item_event(done, 4, reasoning_item("tco_1-0", Some("enc-tco-0"))),
        item_event(added, 5, reasoning_item("tco_1-1", Some("enc-tco-1"))),
        item_event(done, 5, reasoning_item("tco_1-1", Some("enc-tco-1"))),
        item_event(added, 6, reasoning_item("rs_1", None)),
        item_event(done, 6, reasoning_item("rs_1", Some("enc-6"))),
        text_delta("C"),
        text_delta("D"),
        item_event(
            done,
            1,
            message_item(json!([{"type": "output_text", "text": "ABCD"}])),
        ),
        json!({"type": "response.completed", "response": {"id": "resp-1"}}),
    ]
}

fn hosted_item_kinds() -> [BoundaryKind; 10] {
    [
        BoundaryKind::Added("web_search_call"),
        BoundaryKind::Done("web_search_call"),
        BoundaryKind::Added("web_search_call"),
        BoundaryKind::Done("web_search_call"),
        BoundaryKind::Added("reasoning"),
        BoundaryKind::Done("reasoning"),
        BoundaryKind::Added("reasoning"),
        BoundaryKind::Done("reasoning"),
        BoundaryKind::Added("reasoning"),
        BoundaryKind::Done("reasoning"),
    ]
}

fn interleaved_boundary_kinds(dialect: ApiDialect) -> Vec<BoundaryKind> {
    let mut kinds = vec![
        BoundaryKind::Created,
        BoundaryKind::Added("reasoning"),
        BoundaryKind::Done("reasoning"),
        BoundaryKind::Added("message"),
        BoundaryKind::Delta,
        BoundaryKind::Delta,
    ];
    match dialect {
        ApiDialect::OpenAi => {
            kinds.extend(hosted_item_kinds());
            kinds.extend([
                BoundaryKind::Delta,
                BoundaryKind::Delta,
                BoundaryKind::Done("message"),
            ]);
        }
        ApiDialect::Grok => {
            kinds.extend([
                BoundaryKind::Delta,
                BoundaryKind::Delta,
                BoundaryKind::Done("message"),
            ]);
            kinds.extend(hosted_item_kinds());
        }
    }
    kinds.push(BoundaryKind::Completed);
    kinds
}

async fn run_sse_with_dialect(
    events: Vec<serde_json::Value>,
    dialect: ApiDialect,
) -> Vec<ResponseEvent> {
    let mut body = String::new();
    for event in events {
        let kind = event
            .get("type")
            .and_then(|value| value.as_str())
            .expect("fixture event missing type");
        if event.as_object().is_some_and(|object| object.len() == 1) {
            body.push_str(&format!("event: {kind}\n\n"));
        } else {
            body.push_str(&format!("event: {kind}\ndata: {event}\n\n"));
        }
    }

    let (tx, mut rx) = mpsc::channel::<Result<ResponseEvent, ApiError>>(64);
    let stream = ReaderStream::new(std::io::Cursor::new(body))
        .map_err(|err| TransportError::Network(err.to_string()));
    tokio::spawn(process_sse_with_treatment(
        Box::pin(stream),
        tx,
        Duration::from_millis(1000),
        /*telemetry*/ None,
        SafetyBufferingTreatment::default(),
        dialect,
    ));

    let mut out = Vec::new();
    while let Some(event) = rx.recv().await {
        out.push(event.expect("channel closed"));
    }
    out
}

#[tokio::test]
async fn openai_dialect_keeps_interleaved_hosted_stream_in_wire_order() {
    let events = run_sse_with_dialect(interleaved_hosted_sse_events(), ApiDialect::OpenAi).await;
    assert_eq!(
        events.iter().map(boundary_kind).collect::<Vec<_>>(),
        interleaved_boundary_kinds(ApiDialect::OpenAi)
    );
}

#[tokio::test]
async fn grok_dialect_sequences_message_deltas_before_hosted_items() {
    let events = run_sse_with_dialect(interleaved_hosted_sse_events(), ApiDialect::Grok).await;
    assert_eq!(
        events.iter().map(boundary_kind).collect::<Vec<_>>(),
        interleaved_boundary_kinds(ApiDialect::Grok)
    );
}
