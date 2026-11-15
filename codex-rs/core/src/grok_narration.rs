//! Display copy of a Grok thinking block whose signature says it is narration.
//!
//! The reasoning item stays the model-visible record. Narration is a separate
//! commentary message and is not written back as a response item.

use base64::Engine as _;
use codex_protocol::ResponseItemId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;

pub(crate) fn commentary_turn_item(item: &ResponseItem) -> Option<TurnItem> {
    let ResponseItem::Reasoning {
        summary,
        content,
        encrypted_content,
        ..
    } = item
    else {
        return None;
    };
    let signature = encrypted_content.as_deref()?;
    if !signature_marks_narration(signature) {
        return None;
    }
    let text = narration_text(content.as_deref(), summary);
    if text.trim().is_empty() {
        return None;
    }
    Some(TurnItem::AgentMessage(AgentMessageItem {
        id: ResponseItemId::new("msg").to_string(),
        content: vec![AgentMessageContent::Text { text }],
        phase: Some(MessagePhase::Commentary),
        memory_citation: None,
        delivery: None,
        questions: None,
    }))
}

fn narration_text(
    content: Option<&[ReasoningItemContent]>,
    summary: &[ReasoningItemReasoningSummary],
) -> String {
    let from_content = content
        .unwrap_or_default()
        .iter()
        .map(|part| match part {
            ReasoningItemContent::ReasoningText { text } | ReasoningItemContent::Text { text } => {
                text.as_str()
            }
        })
        .collect::<String>();
    if !from_content.trim().is_empty() {
        return from_content;
    }
    summary
        .iter()
        .map(|ReasoningItemReasoningSummary::SummaryText { text }| text.as_str())
        .collect()
}

/// Whether a thinking-block signature names the block as narration.
///
/// Grok writes the kind, `thinking` or `narration`, in the readable header ahead
/// of the opaque payload. A changed header stays ordinary reasoning.
fn signature_marks_narration(signature: &str) -> bool {
    let prefix_len = signature.len().min(48);
    let Some(prefix) = signature.get(..prefix_len) else {
        return false;
    };
    let aligned_len = prefix.len() - prefix.len() % 4;
    let Some(aligned) = prefix.get(..aligned_len) else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(aligned) else {
        return false;
    };
    bytes
        .windows(b"narration".len())
        .any(|window| window == b"narration")
}

#[cfg(test)]
#[path = "grok_narration_tests.rs"]
mod tests;
