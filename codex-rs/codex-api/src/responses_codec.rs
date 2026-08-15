//! Provider-aware encoding and decoding rules for Responses payload items.

use codex_protocol::ResponseItemId;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResponsesDialect {
    #[default]
    OpenAi,
    Grok,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrokImageGenerationWireItem {
    #[serde(rename = "type")]
    _kind: String,
    #[serde(default)]
    id: Option<ResponseItemId>,
    status: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    internal_chat_message_metadata_passthrough: Option<InternalChatMessageMetadataPassthrough>,
}

pub(crate) fn decode_response_item(
    dialect: ResponsesDialect,
    item: Value,
) -> Result<ResponseItem, serde_json::Error> {
    match dialect {
        ResponsesDialect::OpenAi => serde_json::from_value(item),
        ResponsesDialect::Grok => decode_grok_response_item(item),
    }
}

fn decode_grok_response_item(item: Value) -> Result<ResponseItem, serde_json::Error> {
    if item.get("type").and_then(Value::as_str) == Some("image_generation_call") {
        let item: GrokImageGenerationWireItem = serde_json::from_value(item)?;
        return Ok(ResponseItem::GrokImageGenerationCall {
            id: item.id,
            status: item.status,
            prompt: item.prompt,
            result: item.result,
            internal_chat_message_metadata_passthrough: item
                .internal_chat_message_metadata_passthrough,
        });
    }
    let item: ResponseItem = serde_json::from_value(item)?;
    match item {
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::WebSearchCall { .. } => Ok(item),
        ResponseItem::AdditionalTools { .. } => unsupported_grok_output("additional_tools"),
        ResponseItem::AgentMessage { .. } => unsupported_grok_output("agent_message"),
        ResponseItem::LocalShellCall { .. } => unsupported_grok_output("local_shell_call"),
        ResponseItem::ToolSearchCall { .. } => unsupported_grok_output("tool_search_call"),
        ResponseItem::FunctionCallOutput { .. } => unsupported_grok_output("function_call_output"),
        ResponseItem::CustomToolCallOutput { .. } => {
            unsupported_grok_output("custom_tool_call_output")
        }
        ResponseItem::ToolSearchOutput { .. } => unsupported_grok_output("tool_search_output"),
        ResponseItem::ImageGenerationCall { .. } => {
            unsupported_grok_output("openai_image_generation_call")
        }
        ResponseItem::GrokImageGenerationCall { .. } => {
            unsupported_grok_output("internal_grok_image_generation_call")
        }
        ResponseItem::Compaction { .. } => unsupported_grok_output("compaction"),
        ResponseItem::CompactionTrigger { .. } => unsupported_grok_output("compaction_trigger"),
        ResponseItem::ContextCompaction { .. } => unsupported_grok_output("context_compaction"),
        ResponseItem::Other => unsupported_grok_output("unknown"),
    }
}

fn unsupported_grok_output(item_type: &str) -> Result<ResponseItem, serde_json::Error> {
    Err(<serde_json::Error as serde::de::Error>::custom(format!(
        "unsupported Grok response output item type `{item_type}`"
    )))
}

#[cfg(test)]
#[path = "responses_codec_tests.rs"]
mod tests;
