//! Request-only projection from canonical Codex history to Grok Responses input.

use codex_api::ResponsesApiInput;
use codex_protocol::ResponseItemId;
use codex_protocol::error::CodexErr;
use codex_protocol::grok::is_evidence_backed_x_search_name;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use serde::Serialize;

use crate::provider::clear_unprefixed_item_id;

pub(crate) fn project(input: Vec<ResponseItem>) -> Result<ResponsesApiInput, CodexErr> {
    let items = input
        .into_iter()
        .map(project_item)
        .collect::<Result<Vec<_>, _>>()?;
    let wire_items = items
        .iter()
        .map(project_wire_item)
        .collect::<Result<Vec<_>, _>>()?;
    ResponsesApiInput::from_projected(items, wire_items).ok_or_else(|| {
        CodexErr::InvalidRequest("Grok ModelInput projection changed item cardinality".to_string())
    })
}

fn project_item(item: ResponseItem) -> Result<ResponseItem, CodexErr> {
    let is_evidence_backed_x_search = matches!(
        &item,
        ResponseItem::CustomToolCall {
            name,
            namespace: None,
            ..
        } if is_evidence_backed_x_search_name(name)
    );
    let mut projected = if is_evidence_backed_x_search {
        item
    } else {
        match item {
            ResponseItem::Message { .. }
            | ResponseItem::FunctionCallOutput { .. }
            | ResponseItem::WebSearchCall { .. } => item,
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
                ResponseItem::Message {
                    id,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText { text }],
                    phase: None,
                    internal_chat_message_metadata_passthrough,
                }
            }
            ResponseItem::Reasoning {
                id,
                summary,
                content,
                encrypted_content,
                internal_chat_message_metadata_passthrough,
            } => ResponseItem::Reasoning {
                id,
                summary,
                content: Some(content.unwrap_or_default()),
                encrypted_content,
                internal_chat_message_metadata_passthrough,
            },
            ResponseItem::FunctionCall {
                namespace: Some(namespace),
                ..
            } if !namespace.is_empty() => return unsupported("namespaced_function_call"),
            ResponseItem::FunctionCall { .. } => item,
            ResponseItem::GrokImageGenerationCall { .. } => item,
            ResponseItem::AdditionalTools { .. } => return unsupported("additional_tools"),
            ResponseItem::LocalShellCall { .. } => return unsupported("local_shell_call"),
            ResponseItem::ToolSearchCall { .. } => item,
            ResponseItem::CustomToolCall { .. } => return unsupported("custom_tool_call"),
            ResponseItem::CustomToolCallOutput { .. } => {
                return unsupported("custom_tool_call_output");
            }
            ResponseItem::ToolSearchOutput { .. } => item,
            ResponseItem::ImageGenerationCall { .. } => {
                return unsupported("image_generation_call");
            }
            ResponseItem::Compaction { .. } => return unsupported("compaction"),
            ResponseItem::CompactionTrigger { .. } => return unsupported("compaction_trigger"),
            ResponseItem::ContextCompaction { .. } => return unsupported("context_compaction"),
            ResponseItem::Other => return unsupported("unknown"),
        }
    };
    if !matches!(&projected, ResponseItem::GrokImageGenerationCall { .. }) {
        clear_unprefixed_item_id(&mut projected);
    }
    Ok(projected)
}

fn project_wire_item(item: &ResponseItem) -> Result<serde_json::Value, CodexErr> {
    match item {
        ResponseItem::ToolSearchCall {
            call_id,
            status,
            execution,
            arguments,
            ..
        } => {
            if execution != "client" || !matches!(status.as_deref(), None | Some("completed")) {
                return Err(CodexErr::InvalidRequest(
                    "Grok can only replay terminal client tool_search calls".to_string(),
                ));
            }
            let call_id = call_id
                .as_deref()
                .filter(|call_id| !call_id.is_empty())
                .ok_or_else(|| {
                    CodexErr::InvalidRequest(
                        "Grok tool_search history is missing its call id".to_string(),
                    )
                })?;
            let arguments = serde_json::to_string(arguments)?;
            serde_json::to_value(ResponseItem::FunctionCall {
                id: None,
                name: "tool_search".to_string(),
                namespace: None,
                arguments,
                encrypted_function_args: None,
                call_id: call_id.to_string(),
                internal_chat_message_metadata_passthrough: None,
            })
            .map_err(Into::into)
        }
        ResponseItem::ToolSearchOutput {
            call_id,
            status,
            execution,
            tools,
            ..
        } => {
            if execution != "client" || status != "completed" {
                return Err(CodexErr::InvalidRequest(
                    "Grok can only replay completed client tool_search outputs".to_string(),
                ));
            }
            let call_id = call_id
                .as_deref()
                .filter(|call_id| !call_id.is_empty())
                .ok_or_else(|| {
                    CodexErr::InvalidRequest(
                        "Grok tool_search output history is missing its call id".to_string(),
                    )
                })?;
            let output = serde_json::to_string(&serde_json::json!({ "tools": tools }))?;
            serde_json::to_value(ResponseItem::FunctionCallOutput {
                id: None,
                call_id: call_id.to_string(),
                output: FunctionCallOutputPayload::from_text(output),
                internal_chat_message_metadata_passthrough: None,
            })
            .map_err(Into::into)
        }
        ResponseItem::GrokImageGenerationCall {
            id,
            status,
            prompt,
            result,
            ..
        } => {
            if !matches!(
                (status.as_str(), result),
                ("completed", Some(_)) | ("failed", None)
            ) {
                return Err(CodexErr::InvalidRequest(format!(
                    "Grok image history has invalid terminal status/result combination `{status}`"
                )));
            }
            let id = id
                .as_ref()
                .filter(|id| !id.as_str().is_empty())
                .ok_or_else(|| {
                    CodexErr::InvalidRequest(
                        "Grok image history is missing its provider item id".to_string(),
                    )
                })?;
            let prompt = prompt
                .as_deref()
                .filter(|prompt| !prompt.is_empty())
                .ok_or_else(|| {
                    CodexErr::InvalidRequest("Grok image history is missing its prompt".to_string())
                })?;
            serde_json::to_value(GrokImageHistoryInput {
                id,
                item_type: "image_generation_call",
                status,
                prompt,
            })
            .map_err(Into::into)
        }
        _ => serde_json::to_value(item).map_err(Into::into),
    }
}

#[derive(Serialize)]
struct GrokImageHistoryInput<'a> {
    id: &'a ResponseItemId,
    #[serde(rename = "type")]
    item_type: &'static str,
    status: &'a str,
    prompt: &'a str,
}

fn unsupported<T>(item_type: &'static str) -> Result<T, CodexErr> {
    Err(CodexErr::InvalidRequest(format!(
        "Grok does not support Codex history item `{item_type}`"
    )))
}

#[cfg(test)]
#[path = "grok_model_input_tests.rs"]
mod tests;
