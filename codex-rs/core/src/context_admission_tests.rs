use super::*;
use codex_api::ResponsesApiInput;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use serde::Serialize;

#[derive(Serialize)]
struct TestPayload<'a> {
    input: &'a ResponsesApiInput,
}

fn model_with_context_window(context_window: i64) -> ModelInfo {
    let mut model_info = codex_models_manager::model_info::model_info_from_slug("test-model");
    model_info.context_window = Some(context_window);
    model_info.effective_context_window_percent = 100;
    model_info
}

#[test]
fn stock_identity_input_is_not_subject_to_projected_admission() {
    let input: ResponsesApiInput = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }]
    .into();

    ensure_projected_request_fits(
        &TestPayload { input: &input },
        &input,
        &model_with_context_window(1_000),
    )
    .expect("stock identity input keeps the existing Provider admission path");
}

#[test]
fn stock_identity_input_does_not_acquire_grok_item_or_metadata_requirements() {
    let input: ResponsesApiInput = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "A".repeat(40_100),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }]
    .into();

    let mut model_info = model_with_context_window(100_000);
    model_info.context_window = None;
    model_info.max_context_window = None;

    ensure_projected_request_fits(&TestPayload { input: &input }, &input, &model_info)
        .expect("a Grok projection limit must not become a stock OpenAI policy");
}

#[test]
fn bounded_grok_image_projection_does_not_charge_durable_inline_result() {
    let items = vec![ResponseItem::GrokImageGenerationCall {
        id: None,
        status: "completed".to_string(),
        prompt: Some("Draw a fox.".to_string()),
        result: Some("A".repeat(100_000)),
        internal_chat_message_metadata_passthrough: None,
    }];
    let input = ResponsesApiInput::from_projected(
        items,
        vec![serde_json::json!({
            "type": "image_generation_call",
            "status": "completed",
            "prompt": "Draw a fox."
        })],
    )
    .expect("one wire item must correspond to one canonical item");

    ensure_projected_request_fits(
        &TestPayload { input: &input },
        &input,
        &model_with_context_window(1_000),
    )
    .expect("admission must evaluate the bounded request projection");
}

#[test]
fn projected_item_over_hard_limit_is_rejected_even_when_request_fits_window() {
    let items = vec![ResponseItem::GrokImageGenerationCall {
        id: None,
        status: "completed".to_string(),
        prompt: None,
        result: Some("durable".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }];
    let input = ResponsesApiInput::from_projected(
        items,
        vec![serde_json::json!({
            "type": "image_generation_call",
            "status": "completed",
            "prompt": "A".repeat(40_100)
        })],
    )
    .expect("one wire item must correspond to one canonical item");

    let error = ensure_projected_request_fits(
        &TestPayload { input: &input },
        &input,
        &model_with_context_window(100_000),
    )
    .expect_err("an individual projected item must not exceed 10K tokens");

    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::ContextWindowExceeded
    ));
}

#[test]
fn missing_context_window_fails_closed() {
    let items = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }];
    let input = ResponsesApiInput::from_projected(
        items,
        vec![serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]
        })],
    )
    .expect("one wire item must correspond to one canonical item");
    let mut model_info = model_with_context_window(1_000);
    model_info.context_window = None;
    model_info.max_context_window = None;

    let error = ensure_projected_request_fits(&TestPayload { input: &input }, &input, &model_info)
        .expect_err("admission without a context window cannot prove safety");

    assert!(error.to_string().contains("without a context window"));
}
