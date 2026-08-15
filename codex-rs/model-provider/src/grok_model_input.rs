//! Request-only projection from canonical Codex history to Grok Responses input.

use codex_protocol::error::CodexErr;
use codex_protocol::grok::is_evidence_backed_x_search_name;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;

pub(crate) fn project(input: Vec<ResponseItem>) -> Result<Vec<ResponseItem>, CodexErr> {
    input.into_iter().map(project_item).collect()
}

fn project_item(item: ResponseItem) -> Result<ResponseItem, CodexErr> {
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
            let text = plaintext_agent_message_content(&content).ok_or_else(|| {
                CodexErr::InvalidRequest(
                    "Grok cannot replay encrypted collaboration history".to_string(),
                )
            })?;
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
        } if !namespace.is_empty() => unsupported("namespaced_function_call"),
        ResponseItem::FunctionCall { .. } => Ok(item),
        ResponseItem::GrokImageGenerationCall {
            id,
            status,
            prompt,
            result,
            internal_chat_message_metadata_passthrough,
        } => Ok(ResponseItem::GrokImageGenerationWireCall {
            id,
            status,
            prompt,
            result,
            internal_chat_message_metadata_passthrough,
        }),
        ResponseItem::GrokImageGenerationWireCall { .. } => Ok(item),
        ResponseItem::AdditionalTools { .. } => unsupported("additional_tools"),
        ResponseItem::LocalShellCall { .. } => unsupported("local_shell_call"),
        ResponseItem::ToolSearchCall { .. } => unsupported("tool_search_call"),
        ResponseItem::CustomToolCall { .. } => unsupported("custom_tool_call"),
        ResponseItem::CustomToolCallOutput { .. } => unsupported("custom_tool_call_output"),
        ResponseItem::ToolSearchOutput { .. } => unsupported("tool_search_output"),
        ResponseItem::ImageGenerationCall { .. } => unsupported("image_generation_call"),
        ResponseItem::Compaction { .. } => unsupported("compaction"),
        ResponseItem::CompactionTrigger { .. } => unsupported("compaction_trigger"),
        ResponseItem::ContextCompaction { .. } => unsupported("context_compaction"),
        ResponseItem::Other => unsupported("unknown"),
    }
}

fn unsupported<T>(item_type: &'static str) -> Result<T, CodexErr> {
    Err(CodexErr::InvalidRequest(format!(
        "Grok does not support Codex history item `{item_type}`"
    )))
}

#[cfg(test)]
#[path = "grok_model_input_tests.rs"]
mod tests;
