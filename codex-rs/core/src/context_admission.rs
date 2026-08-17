use codex_api::ResponsesApiInput;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::openai_models::ModelInfo;
use codex_utils_output_truncation::approx_tokens_from_byte_count_i64;
use serde::Serialize;
use std::io::Write;

const MAX_PROJECTED_ITEM_TOKENS: i64 = 10_000;

/// Verifies the request payload after Provider-specific history projection.
pub(crate) fn ensure_projected_request_fits<T: Serialize>(
    payload: &T,
    projected_input: &ResponsesApiInput,
    model_info: &ModelInfo,
) -> Result<()> {
    // Identity projection is the stock Provider path. Preserve its existing context-window
    // behavior; this additional proof gate exists only for an actual Provider-owned wire
    // projection whose replay cost can differ from durable history.
    let Some(wire_items) = projected_input.wire_items() else {
        return Ok(());
    };
    let context_window = effective_context_window(model_info).ok_or_else(|| {
        CodexErr::InvalidRequest(format!(
            "cannot admit ModelInput for `{}` without a context window",
            model_info.slug
        ))
    })?;
    let serialized_bytes = serialized_json_len(payload)?;
    let projected_request_tokens = approx_tokens_from_byte_count_i64(serialized_bytes);
    for item in wire_items {
        let item_tokens = approx_tokens_from_byte_count_i64(serialized_json_len(item)?);
        if item_tokens > MAX_PROJECTED_ITEM_TOKENS {
            return Err(CodexErr::ContextWindowExceeded);
        }
    }
    if projected_request_tokens > context_window {
        return Err(CodexErr::ContextWindowExceeded);
    }
    Ok(())
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
