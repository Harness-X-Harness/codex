use super::GrokOutputSequencer;
use crate::sse::ResponsesStreamEvent;
use pretty_assertions::assert_eq;
use serde_json::json;

fn event(kind: &str, output_index: Option<u64>) -> ResponsesStreamEvent {
    let value = match output_index {
        Some(index) => json!({"type": kind, "output_index": index}),
        None => json!({"type": kind}),
    };
    serde_json::from_value(value).expect("ResponsesStreamEvent")
}

fn parse_frames(spec: &str) -> Vec<(String, Option<u64>)> {
    spec.split_whitespace()
        .map(|token| match token.split_once('@') {
            Some((kind, index)) => (kind.to_string(), Some(index.parse().expect("output_index"))),
            None => (token.to_string(), None),
        })
        .collect()
}

fn sequence_spec(spec: &str) -> Vec<(String, Option<u64>)> {
    let mut sequencer = GrokOutputSequencer::default();
    let mut out = Vec::new();
    for (kind, output_index) in parse_frames(spec) {
        out.extend(sequencer.push(event(&kind, output_index)));
    }
    out.into_iter()
        .map(|event| (event.kind, event.output_index))
        .collect()
}

/// Frame order Grok streamed for a hosted `web_search` Turn (recorded
/// 2026-09-16 against `grok.trustedtunnel.app`), reduced to kinds + indices
/// with repeated deltas collapsed. `@n` is `output_index`.
const RECORDED_WIRE: &str = "\
response.created response.in_progress \
response.output_item.added@0 response.reasoning_summary_part.added@0 \
response.reasoning_summary_text.delta@0 response.reasoning_summary_text.done@0 \
response.reasoning_summary_part.done@0 response.output_item.done@0 \
response.output_item.added@1 response.content_part.added@1 \
response.output_text.delta@1 response.output_text.delta@1 \
response.output_item.added@2 response.web_search_call.in_progress@2 \
response.web_search_call.searching@2 response.output_item.added@3 \
response.web_search_call.in_progress@3 response.web_search_call.searching@3 \
response.web_search_call.completed@2 response.output_item.done@2 \
response.output_item.added@4 response.output_item.done@4 \
response.web_search_call.completed@3 response.output_item.done@3 \
response.output_item.added@5 response.output_item.done@5 \
response.output_item.added@6 response.output_item.done@6 \
response.output_text.delta@1 response.output_text.delta@1 \
response.output_text.annotation.added@1 response.output_text.done@1 \
response.content_part.done@1 response.output_item.done@1 \
response.completed";

const RECORDED_SEQUENCED: &str = "\
response.created response.in_progress \
response.output_item.added@0 response.reasoning_summary_part.added@0 \
response.reasoning_summary_text.delta@0 response.reasoning_summary_text.done@0 \
response.reasoning_summary_part.done@0 response.output_item.done@0 \
response.output_item.added@1 response.content_part.added@1 \
response.output_text.delta@1 response.output_text.delta@1 \
response.output_text.delta@1 response.output_text.delta@1 \
response.output_text.annotation.added@1 response.output_text.done@1 \
response.content_part.done@1 response.output_item.done@1 \
response.output_item.added@2 response.web_search_call.in_progress@2 \
response.web_search_call.searching@2 response.web_search_call.completed@2 \
response.output_item.done@2 response.output_item.added@3 \
response.web_search_call.in_progress@3 response.web_search_call.searching@3 \
response.web_search_call.completed@3 response.output_item.done@3 \
response.output_item.added@4 response.output_item.done@4 \
response.output_item.added@5 response.output_item.done@5 \
response.output_item.added@6 response.output_item.done@6 \
response.completed";

#[test]
fn recorded_interleaved_stream_sequences_message_before_hosted_items() {
    assert_eq!(
        sequence_spec(RECORDED_WIRE),
        parse_frames(RECORDED_SEQUENCED)
    );
}

#[test]
fn sequential_stream_is_identity() {
    let spec = "\
response.created \
response.output_item.added@0 \
response.output_item.done@0 \
response.output_item.added@1 \
response.output_text.delta@1 \
response.output_item.done@1 \
response.output_item.added@2 \
response.output_item.done@2 \
response.completed";
    assert_eq!(sequence_spec(spec), parse_frames(spec));
}

#[test]
fn late_lower_index_frame_passes_through_while_higher_head_is_open() {
    let spec = "\
response.output_item.added@5 \
response.output_text.delta@1 \
response.output_item.done@5";
    assert_eq!(sequence_spec(spec), parse_frames(spec));
}

#[test]
fn completed_drains_open_head_pending_items_in_index_order() {
    assert_eq!(
        sequence_spec(
            "\
response.output_item.added@1 \
response.output_item.added@3 \
response.output_item.done@3 \
response.output_item.added@2 \
response.output_item.done@2 \
response.completed"
        ),
        parse_frames(
            "\
response.output_item.added@1 \
response.output_item.added@2 \
response.output_item.done@2 \
response.output_item.added@3 \
response.output_item.done@3 \
response.completed"
        )
    );
}

#[test]
fn indexless_frames_are_never_held_while_a_head_is_open() {
    let spec = "\
response.output_item.added@1 \
response.in_progress \
response.output_text.delta@1 \
response.output_item.done@1 \
response.completed";
    assert_eq!(sequence_spec(spec), parse_frames(spec));
}
