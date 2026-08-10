use super::*;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn grok_decoder_preserves_native_image_prompt() {
    let item = decode_response_item(
        ResponsesDialect::Grok,
        json!({
            "type": "image_generation_call",
            "id": "ig_1",
            "status": "completed",
            "prompt": "Draw a fox",
            "result": "base64-image"
        }),
    )
    .expect("Grok image item should decode");

    assert_eq!(
        item,
        ResponseItem::GrokImageGenerationCall {
            id: Some(codex_protocol::ResponseItemId::with_suffix("ig", "1")),
            status: "completed".to_string(),
            prompt: Some("Draw a fox".to_string()),
            result: Some("base64-image".to_string()),
            internal_chat_message_metadata_passthrough: None,
        }
    );
}

#[test]
fn openai_decoder_does_not_guess_grok_from_image_shape() {
    let result = decode_response_item(
        ResponsesDialect::OpenAi,
        json!({
            "type": "image_generation_call",
            "id": "ig_1",
            "status": "in_progress",
            "prompt": "Draw a fox",
            "result": null
        }),
    );

    assert!(result.is_err());
}

#[test]
fn unknown_output_is_strict_only_for_grok() {
    let item = json!({"type": "future_provider_item", "value": 1});

    assert_eq!(
        decode_response_item(ResponsesDialect::OpenAi, item.clone())
            .expect("OpenAI parser should preserve forward compatibility"),
        ResponseItem::Other
    );
    assert!(decode_response_item(ResponsesDialect::Grok, item).is_err());
}

#[test]
fn grok_decoder_rejects_codex_only_output_variants() {
    let result = decode_response_item(
        ResponsesDialect::Grok,
        json!({
            "type": "tool_search_call",
            "call_id": "search-1",
            "status": "completed",
            "execution": "client",
            "arguments": {"query": "calendar"}
        }),
    );

    assert!(result.is_err());
}
