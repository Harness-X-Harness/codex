use super::*;
use codex_protocol::models::ContentItem;
use serde::Serialize;

#[derive(Serialize)]
struct TestPayload<'a> {
    input: &'a [ResponseItem],
}

fn model_with_context_window(context_window: i64) -> ModelInfo {
    let mut model_info = codex_models_manager::model_info::model_info_from_slug("test-model");
    model_info.context_window = Some(context_window);
    model_info.effective_context_window_percent = 100;
    model_info
}

#[test]
fn ordinary_projected_input_within_budget_is_admitted() {
    let input = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }];

    ensure_projected_request_fits(
        &TestPayload { input: &input },
        &input,
        &model_with_context_window(1_000),
    )
    .expect("small projected input should fit");
}

#[test]
fn inline_grok_image_result_uses_conservative_replay_cost() {
    let input = vec![ResponseItem::GrokImageGenerationWireCall {
        id: None,
        status: "completed".to_string(),
        prompt: None,
        result: Some("A".repeat(1_000)),
        internal_chat_message_metadata_passthrough: None,
    }];

    let error = ensure_projected_request_fits(
        &TestPayload { input: &input },
        &input,
        &model_with_context_window(900),
    )
    .expect_err("opaque inline replay must not use the ordinary four-byte estimate");

    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::ContextWindowExceeded
    ));
}

#[test]
fn projected_item_over_hard_limit_is_rejected_even_when_request_fits_window() {
    let input = vec![ResponseItem::GrokImageGenerationWireCall {
        id: None,
        status: "completed".to_string(),
        prompt: None,
        result: Some("A".repeat(10_001)),
        internal_chat_message_metadata_passthrough: None,
    }];

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
    let input = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }];
    let mut model_info = model_with_context_window(1_000);
    model_info.context_window = None;
    model_info.max_context_window = None;

    let error = ensure_projected_request_fits(&TestPayload { input: &input }, &input, &model_info)
        .expect_err("admission without a context window cannot prove safety");

    assert!(error.to_string().contains("without a context window"));
}
