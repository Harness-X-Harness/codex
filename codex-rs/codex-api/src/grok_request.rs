//! Construct Grok Responses egress from a whitelist of fields with evidence.

use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::common::TextFormat;
use codex_protocol::ResponseItemId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// Why a Grok Responses request cannot be constructed from the canonical input.
#[derive(Debug, thiserror::Error)]
pub(crate) enum GrokProjectionError {
    #[error("Grok cannot replay {variant} at input[{index}]")]
    RejectedItem { index: usize, variant: &'static str },
    #[error("Grok cannot replay tool type {tool_type} at tools[{index}]")]
    RejectedTool { index: usize, tool_type: String },
    #[error("Grok cannot replay unsupported encrypted collaboration history at input[{index}]")]
    EncryptedCollaborationHistory { index: usize },
    #[error("Grok cannot replay function_call_output history without call_id at input[{index}]")]
    FunctionCallOutputMissingCallId { index: usize },
    #[error("{0}")]
    Serialize(#[from] serde_json::Error),
}

pub(crate) fn build(request: &ResponsesApiRequest) -> Result<Value, GrokProjectionError> {
    Ok(serde_json::to_value(
        &GrokResponsesRequest::try_from_request(request)?,
    )?)
}

#[derive(Serialize)]
struct GrokResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'a str>,
    input: Vec<GrokInputItem<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GrokTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    reasoning: Option<GrokReasoning<'a>>,
    store: bool,
    stream: bool,
    include: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<GrokTextControls<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_metadata: Option<&'a HashMap<String, String>>,
}

#[derive(Serialize)]
struct GrokReasoning<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'a ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a ReasoningSummary>,
}

#[derive(Serialize)]
struct GrokTextControls<'a> {
    format: &'a TextFormat,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GrokInputItem<'a> {
    Message {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        role: &'a str,
        content: Vec<GrokContentItem>,
    },
    Reasoning(GrokReasoningItem<'a>),
    FunctionCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        name: &'a str,
        arguments: &'a str,
        call_id: &'a str,
    },
    FunctionCallOutput(GrokFunctionCallOutput<'a>),
    CustomToolCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        call_id: &'a str,
        name: &'a str,
        input: &'a str,
    },
    CustomToolCallOutput {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        call_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<&'a str>,
        output: &'a FunctionCallOutputPayload,
    },
    WebSearchCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<&'a WebSearchAction>,
    },
    ImageGenerationCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<&'a str>,
        result: &'a str,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GrokContentItem {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    InputAudio {
        audio_url: String,
    },
    OutputText {
        text: String,
    },
}

#[derive(Serialize)]
struct GrokReasoningItem<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    summary: &'a [ReasoningItemReasoningSummary],
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a [ReasoningItemContent]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_content: Option<&'a str>,
}

#[derive(Serialize)]
struct GrokFunctionCallOutput<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    call_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    output: &'a FunctionCallOutputPayload,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GrokTool {
    Function {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
    Custom {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<Value>,
    },
    WebSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<GrokWebSearchFilters>,
    },
    XSearch {},
}

#[derive(Serialize)]
struct GrokWebSearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_domains: Option<Vec<String>>,
}

impl<'a> GrokResponsesRequest<'a> {
    fn try_from_request(request: &'a ResponsesApiRequest) -> Result<Self, GrokProjectionError> {
        let tools = project_tools(request.tools.as_ref())?;
        let (tool_choice, parallel_tool_calls) = match &tools {
            Some(_) => (
                Some(request.tool_choice.as_str()),
                Some(request.parallel_tool_calls),
            ),
            None => (None, None),
        };
        Ok(Self {
            model: &request.model,
            instructions: (!request.instructions.is_empty())
                .then_some(request.instructions.as_str()),
            input: project_input(&request.input)?,
            tools,
            tool_choice,
            parallel_tool_calls,
            reasoning: request.reasoning.as_ref().map(|reasoning| GrokReasoning {
                effort: reasoning.effort.as_ref(),
                summary: reasoning.summary.as_ref(),
            }),
            store: request.store,
            stream: request.stream,
            include: &request.include,
            prompt_cache_key: request.prompt_cache_key.as_deref(),
            text: request
                .text
                .as_ref()
                .and_then(|text| text.format.as_ref())
                .map(|format| GrokTextControls { format }),
            client_metadata: request.client_metadata.as_ref(),
        })
    }
}

fn item_id(id: &Option<ResponseItemId>) -> Option<&str> {
    id.as_deref()
}

fn reject_item(index: usize, variant: &'static str) -> GrokProjectionError {
    GrokProjectionError::RejectedItem { index, variant }
}

fn project_input(input: &[ResponseItem]) -> Result<Vec<GrokInputItem<'_>>, GrokProjectionError> {
    let mut items = Vec::with_capacity(input.len());
    for (index, item) in input.iter().enumerate() {
        match item {
            ResponseItem::Message {
                id, role, content, ..
            } => {
                items.push(GrokInputItem::Message {
                    id: item_id(id),
                    role,
                    content: grok_content_items(content),
                });
            }
            ResponseItem::AgentMessage { id, content, .. } => {
                let Some(text) = plaintext_agent_message_content(content) else {
                    return Err(GrokProjectionError::EncryptedCollaborationHistory { index });
                };
                items.push(GrokInputItem::Message {
                    id: item_id(id),
                    role: "user",
                    content: vec![GrokContentItem::InputText { text }],
                });
            }
            ResponseItem::Reasoning {
                id,
                summary,
                content,
                encrypted_content,
                ..
            } => items.push(project_reasoning(id, summary, content, encrypted_content)),
            ResponseItem::FunctionCall {
                id,
                name,
                arguments,
                call_id,
                ..
            } => items.push(GrokInputItem::FunctionCall {
                id: item_id(id),
                name,
                arguments,
                call_id,
            }),
            ResponseItem::FunctionCallOutput {
                id,
                call_id,
                name,
                output,
                ..
            } => items.push(project_function_call_output(
                index, id, call_id, name, output,
            )?),
            ResponseItem::CustomToolCall {
                id,
                call_id,
                name,
                input,
                ..
            } => items.push(GrokInputItem::CustomToolCall {
                id: item_id(id),
                call_id,
                name,
                input,
            }),
            ResponseItem::CustomToolCallOutput {
                id,
                call_id,
                name,
                output,
                ..
            } => items.push(GrokInputItem::CustomToolCallOutput {
                id: item_id(id),
                call_id,
                name: name.as_deref(),
                output,
            }),
            ResponseItem::WebSearchCall { id, action, .. } => {
                items.push(GrokInputItem::WebSearchCall {
                    id: item_id(id),
                    action: action.as_ref(),
                });
            }
            ResponseItem::ImageGenerationCall {
                id,
                revised_prompt,
                result,
                ..
            } => items.push(GrokInputItem::ImageGenerationCall {
                id: item_id(id),
                revised_prompt: revised_prompt.as_deref(),
                result,
            }),
            ResponseItem::CompactionTrigger {} | ResponseItem::Other => {}
            ResponseItem::ConfigurationUpdate { .. } => {
                return Err(reject_item(index, "configuration_update"));
            }
            ResponseItem::Compaction { .. } => return Err(reject_item(index, "compaction")),
            ResponseItem::ContextCompaction { .. } => {
                return Err(reject_item(index, "context_compaction"));
            }
            ResponseItem::LocalShellCall { .. } => {
                return Err(reject_item(index, "local_shell_call"));
            }
            ResponseItem::ToolSearchCall { .. } => {
                return Err(reject_item(index, "tool_search_call"));
            }
            ResponseItem::ToolSearchOutput { .. } => {
                return Err(reject_item(index, "tool_search_output"));
            }
            ResponseItem::AdditionalTools { .. } => {
                return Err(reject_item(index, "additional_tools"));
            }
        }
    }
    Ok(items)
}

fn grok_content_items(content: &[ContentItem]) -> Vec<GrokContentItem> {
    content
        .iter()
        .map(|item| match item {
            ContentItem::InputText { text } => GrokContentItem::InputText { text: text.clone() },
            ContentItem::InputImage { image_url, detail } => GrokContentItem::InputImage {
                image_url: image_url.clone(),
                detail: *detail,
            },
            ContentItem::InputAudio { audio_url } => GrokContentItem::InputAudio {
                audio_url: audio_url.clone(),
            },
            ContentItem::OutputText { text } => GrokContentItem::OutputText { text: text.clone() },
        })
        .collect()
}

fn project_reasoning<'a>(
    id: &'a Option<ResponseItemId>,
    summary: &'a [ReasoningItemReasoningSummary],
    content: &'a Option<Vec<ReasoningItemContent>>,
    encrypted_content: &'a Option<String>,
) -> GrokInputItem<'a> {
    // Some("") still counts as a blob field: stock pre-mapping wipes `content`
    // whenever `encrypted_content` is `Some`, then the denylist omits an empty blob.
    let has_encrypted_field = encrypted_content.is_some();
    let usable_blob = encrypted_content
        .as_deref()
        .is_some_and(|blob| !blob.is_empty());
    let well_typed_content = content.as_ref().is_some_and(|items| {
        items
            .iter()
            .any(|item| matches!(item, ReasoningItemContent::ReasoningText { .. }))
    });
    GrokInputItem::Reasoning(GrokReasoningItem {
        id: item_id(id),
        summary,
        content: if has_encrypted_field || !well_typed_content {
            None
        } else {
            content.as_deref()
        },
        encrypted_content: if usable_blob {
            encrypted_content.as_deref()
        } else {
            None
        },
    })
}

fn project_function_call_output<'a>(
    index: usize,
    id: &'a Option<ResponseItemId>,
    call_id: &'a Option<String>,
    name: &'a Option<String>,
    output: &'a FunctionCallOutputPayload,
) -> Result<GrokInputItem<'a>, GrokProjectionError> {
    if let Some(call_id) = call_id.as_deref().filter(|call_id| !call_id.is_empty()) {
        return Ok(GrokInputItem::FunctionCallOutput(GrokFunctionCallOutput {
            id: item_id(id),
            call_id,
            name: name.as_deref(),
            output,
        }));
    }

    if name.as_deref().is_some_and(|name| !name.is_empty())
        && let Some(text) = output.text_content()
    {
        return Ok(GrokInputItem::Message {
            id: item_id(id),
            role: "user",
            content: vec![GrokContentItem::InputText {
                text: text.to_string(),
            }],
        });
    }

    Err(GrokProjectionError::FunctionCallOutputMissingCallId { index })
}

fn project_tools(
    tools: Option<&ResponsesApiTools>,
) -> Result<Option<Vec<GrokTool>>, GrokProjectionError> {
    let Some(tools) = tools else {
        return Ok(None);
    };
    let parsed: Value = serde_json::from_str(tools.as_raw_value().get())?;
    let Value::Array(array) = parsed else {
        return Err(GrokProjectionError::RejectedTool {
            index: 0,
            tool_type: "unknown".to_string(),
        });
    };
    if array.is_empty() {
        return Ok(None);
    }

    let mut projected = Vec::with_capacity(array.len() + 1);
    let mut has_x_search = false;
    for (index, tool) in array.iter().enumerate() {
        let tool_type = tool
            .get("type")
            .and_then(Value::as_str)
            .filter(|tool_type| !tool_type.is_empty())
            .unwrap_or("unknown");
        match tool_type {
            "function" => projected.push(project_function_tool(tool)),
            "custom" => projected.push(project_custom_tool(tool)),
            "web_search" => projected.push(GrokTool::WebSearch { filters: None }),
            "x_search" => {
                has_x_search = true;
                projected.push(GrokTool::XSearch {});
            }
            other => {
                return Err(GrokProjectionError::RejectedTool {
                    index,
                    tool_type: other.to_string(),
                });
            }
        }
    }
    if !has_x_search {
        projected.push(GrokTool::XSearch {});
    }
    Ok(Some(projected))
}

fn project_function_tool(tool: &Value) -> GrokTool {
    GrokTool::Function {
        name: json_string(tool, "name").unwrap_or_default(),
        description: json_string(tool, "description"),
        parameters: json_value(tool, "parameters"),
        strict: tool.get("strict").and_then(Value::as_bool),
    }
}

fn project_custom_tool(tool: &Value) -> GrokTool {
    GrokTool::Custom {
        name: json_string(tool, "name").unwrap_or_default(),
        description: json_string(tool, "description"),
        format: json_value(tool, "format"),
    }
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn json_value(value: &Value, key: &str) -> Option<Value> {
    value.get(key).cloned().filter(|value| !value.is_null())
}

#[cfg(test)]
#[path = "grok_request_tests.rs"]
mod tests;
