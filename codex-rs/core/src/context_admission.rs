use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_utils_output_truncation::approx_tokens_from_byte_count_i64;
use serde::Serialize;
use std::io::Write;

use crate::context_manager::estimate_item_token_count;

const MAX_PROJECTED_ITEM_TOKENS: i64 = 10_000;

/// Verifies the request payload after Provider-specific history projection.
pub(crate) fn ensure_projected_request_fits<T: Serialize>(
    payload: &T,
    projected_input: &[ResponseItem],
    model_info: &ModelInfo,
) -> Result<()> {
    let context_window = effective_context_window(model_info).ok_or_else(|| {
        CodexErr::InvalidRequest(format!(
            "cannot admit ModelInput for `{}` without a context window",
            model_info.slug
        ))
    })?;
    let serialized_bytes = serialized_json_len(payload)?;
    let serialized_tokens = approx_tokens_from_byte_count_i64(serialized_bytes);
    let raw_input_tokens = projected_input
        .iter()
        .map(|item| {
            serialized_json_len(item)
                .map(approx_tokens_from_byte_count_i64)
                .unwrap_or(i64::MAX)
        })
        .fold(0i64, i64::saturating_add);
    let mut projected_input_tokens = 0i64;
    for item in projected_input {
        let item_tokens = estimate_projected_item_token_count(item);
        if item_tokens > MAX_PROJECTED_ITEM_TOKENS {
            return Err(CodexErr::ContextWindowExceeded);
        }
        projected_input_tokens = projected_input_tokens.saturating_add(item_tokens);
    }
    let projected_request_tokens = serialized_tokens
        .saturating_sub(raw_input_tokens)
        .saturating_add(projected_input_tokens);
    if projected_request_tokens > context_window {
        return Err(CodexErr::ContextWindowExceeded);
    }
    Ok(())
}

fn estimate_projected_item_token_count(item: &ResponseItem) -> i64 {
    let ResponseItem::GrokImageGenerationWireCall {
        result: Some(result),
        ..
    } = item
    else {
        return estimate_item_token_count(item);
    };
    let raw = serialized_json_len(item).unwrap_or(i64::MAX);
    // Live Gateway evidence shows inline image replay tokenizes much more densely than ordinary
    // prose. One token per serialized result byte is deliberately conservative.
    approx_tokens_from_byte_count_i64(
        raw.saturating_add(
            i64::try_from(result.len())
                .unwrap_or(i64::MAX)
                .saturating_mul(3),
        ),
    )
}

fn serialized_json_len<T: Serialize>(value: &T) -> Result<i64> {
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value)?;
    Ok(i64::try_from(writer.bytes).unwrap_or(i64::MAX))
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn effective_context_window(model_info: &ModelInfo) -> Option<i64> {
    model_info.resolved_context_window().map(|context_window| {
        context_window.saturating_mul(model_info.effective_context_window_percent) / 100
    })
}

#[cfg(test)]
#[path = "context_admission_tests.rs"]
mod tests;
