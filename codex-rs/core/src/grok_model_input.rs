//! Request-only projection from canonical Codex history to Grok Responses input.

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_tools::is_evidence_backed_x_search_name;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum GrokModelInputError {
    #[error("Grok cannot replay encrypted collaboration history")]
    EncryptedAgentMessage,
    #[error("Grok does not support Codex history item `{0}`")]
    UnsupportedItem(&'static str),
}

/// Encodes canonical Codex history for one Grok request without changing durable state.
pub(crate) fn encode(input: Vec<ResponseItem>) -> Result<Vec<ResponseItem>, GrokModelInputError> {
    input.into_iter().map(encode_item).collect()
}

fn encode_item(item: ResponseItem) -> Result<ResponseItem, GrokModelInputError> {
    if matches!(
        &item,
        ResponseItem::CustomToolCall {
            name,
            namespace: None,
            ..
        } if is_evidence_backed_x_search_name(name)
    ) {
        return Ok(item);
    }
    match item {
        ResponseItem::Message { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::WebSearchCall { .. } => Ok(item),
        ResponseItem::AgentMessage {
            id,
            content,
            internal_chat_message_metadata_passthrough,
            ..
        } => {
            let text = plaintext_agent_message_content(&content)
                .ok_or(GrokModelInputError::EncryptedAgentMessage)?;
            Ok(ResponseItem::Message {
                id,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text }],
                phase: None,
                internal_chat_message_metadata_passthrough,
            })
        }
        ResponseItem::Reasoning {
            id,
            summary,
            content,
            encrypted_content,
            internal_chat_message_metadata_passthrough,
        } => Ok(ResponseItem::Reasoning {
            id,
            summary,
            content: Some(content.unwrap_or_default()),
            encrypted_content,
            internal_chat_message_metadata_passthrough,
        }),
        ResponseItem::FunctionCall {
            namespace: Some(namespace),
            ..
        } if !namespace.is_empty() => Err(GrokModelInputError::UnsupportedItem(
            "namespaced_function_call",
        )),
        ResponseItem::FunctionCall { .. } => Ok(item),
        ResponseItem::AdditionalTools { .. } => {
            Err(GrokModelInputError::UnsupportedItem("additional_tools"))
        }
        ResponseItem::LocalShellCall { .. } => {
            Err(GrokModelInputError::UnsupportedItem("local_shell_call"))
        }
        ResponseItem::ToolSearchCall { .. } => {
            Err(GrokModelInputError::UnsupportedItem("tool_search_call"))
        }
        ResponseItem::CustomToolCall { .. } => {
            Err(GrokModelInputError::UnsupportedItem("custom_tool_call"))
        }
        ResponseItem::CustomToolCallOutput { .. } => Err(GrokModelInputError::UnsupportedItem(
            "custom_tool_call_output",
        )),
        ResponseItem::ToolSearchOutput { .. } => {
            Err(GrokModelInputError::UnsupportedItem("tool_search_output"))
        }
        ResponseItem::ImageGenerationCall { .. } => Err(GrokModelInputError::UnsupportedItem(
            "image_generation_call",
        )),
        ResponseItem::Compaction { .. } => Err(GrokModelInputError::UnsupportedItem("compaction")),
        ResponseItem::CompactionTrigger { .. } => {
            Err(GrokModelInputError::UnsupportedItem("compaction_trigger"))
        }
        ResponseItem::ContextCompaction { .. } => {
            Err(GrokModelInputError::UnsupportedItem("context_compaction"))
        }
        ResponseItem::Other => Err(GrokModelInputError::UnsupportedItem("unknown")),
    }
}

#[cfg(test)]
#[path = "grok_model_input_tests.rs"]
mod tests;
