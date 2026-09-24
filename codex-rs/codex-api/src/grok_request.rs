//! Construct Grok Responses egress from a whitelist of fields with evidence.
//!
//! `try_from_request` destructures `ResponsesApiRequest` exhaustively so a new
//! stock field fails compilation until it is classified as emit, map, omit, or
//! reject. Supported `ResponseItem` variants do the same for their fields.

use crate::common::Reasoning;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::common::TextControls;
use crate::common::TextFormat;
use crate::provider::Provider;
use crate::provider::XSearchProviderConfig;
use codex_protocol::ResponseItemId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Serialize;
use serde_json::Value;

/// Verified Grok hosted `web_search` domain-list limit.
const GROK_WEB_SEARCH_DOMAIN_LIMIT: usize = 5;

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
    #[error("Grok cannot apply unsupported search restriction: {0}")]
    UnsupportedSearchRestriction(String),
    #[error("{0}")]
    Serialize(#[from] serde_json::Error),
}

pub(crate) fn build(
    request: &ResponsesApiRequest,
    provider: &Provider,
) -> Result<Value, GrokProjectionError> {
    Ok(serde_json::to_value(
        &GrokResponsesRequest::try_from_request(request, provider.x_search.as_ref())?,
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
    reasoning: Option<GrokReasoning<'a>>,
    stream: bool,
    include: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<GrokTextControls<'a>>,
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
    XSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        from_date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        to_date: Option<String>,
    },
}

#[derive(Serialize)]
struct GrokWebSearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    excluded_domains: Option<Vec<String>>,
}

impl<'a> GrokResponsesRequest<'a> {
    fn try_from_request(
        request: &'a ResponsesApiRequest,
        x_search: Option<&XSearchProviderConfig>,
    ) -> Result<Self, GrokProjectionError> {
        let ResponsesApiRequest {
            model,
            instructions,
            input,
            tools,
            tool_choice,
            parallel_tool_calls: _,
            reasoning,
            store: _,
            stream,
            stream_options: _,
            include,
            service_tier: _,
            prompt_cache_key,
            text,
            client_metadata: _,
            access_programs: _,
        } = request;
        let tools = project_tools(tools.as_ref(), x_search)?;
        let tool_choice = tools.is_some().then_some(tool_choice.as_str());
        Ok(Self {
            model,
            instructions: (!instructions.is_empty()).then_some(instructions.as_str()),
            input: project_input(input)?,
            tools,
            tool_choice,
            reasoning: reasoning.as_ref().map(|reasoning| {
                let Reasoning {
                    effort,
                    summary,
                    context: _,
                } = reasoning;
                GrokReasoning {
                    effort: effort.as_ref(),
                    summary: summary.as_ref(),
                }
            }),
            stream: *stream,
            include,
            prompt_cache_key: prompt_cache_key.as_deref(),
            text: text.as_ref().and_then(|text| {
                let TextControls {
                    verbosity: _,
                    format,
                } = text;
                format.as_ref().map(|format| GrokTextControls { format })
            }),
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
                id,
                role,
                content,
                phase: _,
                internal_chat_message_metadata_passthrough: _,
            } => {
                items.push(GrokInputItem::Message {
                    id: item_id(id),
                    role,
                    content: grok_content_items(index, content)?,
                });
            }
            ResponseItem::AgentMessage {
                id,
                author: _,
                recipient: _,
                content,
                internal_chat_message_metadata_passthrough: _,
            } => {
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
                internal_chat_message_metadata_passthrough: _,
            } => items.push(project_reasoning(id, summary, content, encrypted_content)),
            ResponseItem::FunctionCall {
                id,
                name,
                namespace: _,
                arguments,
                encrypted_function_args: _,
                call_id,
                internal_chat_message_metadata_passthrough: _,
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
                namespace: _,
                output,
                internal_chat_message_metadata_passthrough: _,
            } => items.push(project_function_call_output(
                index, id, call_id, name, output,
            )?),
            ResponseItem::CustomToolCall {
                id,
                status: _,
                call_id,
                name,
                namespace: _,
                input,
                internal_chat_message_metadata_passthrough: _,
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
                internal_chat_message_metadata_passthrough: _,
            } => items.push(GrokInputItem::CustomToolCallOutput {
                id: item_id(id),
                call_id,
                name: name.as_deref(),
                output,
            }),
            ResponseItem::WebSearchCall {
                id,
                status: _,
                action,
                internal_chat_message_metadata_passthrough: _,
            } => {
                items.push(GrokInputItem::WebSearchCall {
                    id: item_id(id),
                    action: action.as_ref(),
                });
            }
            ResponseItem::ImageGenerationCall {
                id,
                status: _,
                revised_prompt,
                result,
                internal_chat_message_metadata_passthrough: _,
            } => items.push(GrokInputItem::ImageGenerationCall {
                id: item_id(id),
                revised_prompt: revised_prompt.as_deref(),
                result,
            }),
            ResponseItem::CompactionTrigger {} | ResponseItem::Other => {}
            ResponseItem::ConfigurationUpdate { reasoning: _ } => {
                return Err(reject_item(index, "configuration_update"));
            }
            ResponseItem::Compaction {
                id: _,
                encrypted_content: _,
                internal_chat_message_metadata_passthrough: _,
            } => return Err(reject_item(index, "compaction")),
            ResponseItem::ContextCompaction {
                id: _,
                encrypted_content: _,
                internal_chat_message_metadata_passthrough: _,
            } => {
                return Err(reject_item(index, "context_compaction"));
            }
            ResponseItem::LocalShellCall {
                id: _,
                call_id: _,
                status: _,
                action: _,
                internal_chat_message_metadata_passthrough: _,
            } => {
                return Err(reject_item(index, "local_shell_call"));
            }
            ResponseItem::ToolSearchCall {
                id: _,
                call_id: _,
                status: _,
                execution: _,
                arguments: _,
                internal_chat_message_metadata_passthrough: _,
            } => {
                return Err(reject_item(index, "tool_search_call"));
            }
            ResponseItem::ToolSearchOutput {
                id: _,
                call_id: _,
                status: _,
                execution: _,
                tools: _,
                internal_chat_message_metadata_passthrough: _,
            } => {
                return Err(reject_item(index, "tool_search_output"));
            }
            ResponseItem::AdditionalTools {
                id: _,
                role: _,
                tools: _,
            } => {
                return Err(reject_item(index, "additional_tools"));
            }
        }
    }
    Ok(items)
}

fn grok_content_items(
    index: usize,
    content: &[ContentItem],
) -> Result<Vec<GrokContentItem>, GrokProjectionError> {
    content
        .iter()
        .map(|item| match item {
            ContentItem::InputText { text } => {
                Ok(GrokContentItem::InputText { text: text.clone() })
            }
            ContentItem::InputImage {
                image: ImageReference::Inline { image_url },
                detail,
            } => Ok(GrokContentItem::InputImage {
                image_url: image_url.clone(),
                detail: *detail,
            }),
            ContentItem::InputImage {
                image: ImageReference::File { file_id: _ },
                detail: _,
            } => Err(reject_item(index, "input_image.file_id")),
            ContentItem::InputAudio { audio_url } => Ok(GrokContentItem::InputAudio {
                audio_url: audio_url.clone(),
            }),
            ContentItem::OutputText { text } => {
                Ok(GrokContentItem::OutputText { text: text.clone() })
            }
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
    x_search: Option<&XSearchProviderConfig>,
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
            "web_search" => projected.push(project_web_search_tool(tool)?),
            "x_search" => {
                has_x_search = true;
                projected.push(GrokTool::XSearch {
                    from_date: x_search_ymd(tool, "from_date")?
                        .or_else(|| x_search.and_then(|window| window.from_date.clone())),
                    to_date: x_search_ymd(tool, "to_date")?
                        .or_else(|| x_search.and_then(|window| window.to_date.clone())),
                });
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
        projected.push(GrokTool::XSearch {
            from_date: x_search.and_then(|window| window.from_date.clone()),
            to_date: x_search.and_then(|window| window.to_date.clone()),
        });
    }
    Ok(Some(projected))
}

fn project_web_search_tool(tool: &Value) -> Result<GrokTool, GrokProjectionError> {
    let allowed_domains = project_web_search_domains(tool, "allowed_domains")?;
    let excluded_domains = project_web_search_domains(tool, "excluded_domains")?;
    let filters = match (allowed_domains, excluded_domains) {
        (None, None) => None,
        (Some(_), Some(_)) => {
            return Err(GrokProjectionError::UnsupportedSearchRestriction(
                "web_search filters cannot include both allowed_domains and excluded_domains"
                    .to_string(),
            ));
        }
        (Some(allowed_domains), None) => Some(GrokWebSearchFilters {
            allowed_domains: Some(allowed_domains),
            excluded_domains: None,
        }),
        (None, Some(excluded_domains)) => Some(GrokWebSearchFilters {
            allowed_domains: None,
            excluded_domains: Some(excluded_domains),
        }),
    };
    Ok(GrokTool::WebSearch { filters })
}

fn project_web_search_domains(
    tool: &Value,
    key: &str,
) -> Result<Option<Vec<String>>, GrokProjectionError> {
    let Some(domains) = tool.get("filters").and_then(|filters| filters.get(key)) else {
        return Ok(None);
    };
    let Some(array) = domains.as_array() else {
        return Err(GrokProjectionError::UnsupportedSearchRestriction(format!(
            "web_search filters.{key} must be an array of strings"
        )));
    };
    if array.is_empty() {
        return Ok(None);
    }
    if array.len() > GROK_WEB_SEARCH_DOMAIN_LIMIT {
        return Err(GrokProjectionError::UnsupportedSearchRestriction(format!(
            "web_search filters.{key} cannot list more than {GROK_WEB_SEARCH_DOMAIN_LIMIT} domains"
        )));
    }
    let mut projected = Vec::with_capacity(array.len());
    for domain in array {
        let Some(domain) = domain.as_str() else {
            return Err(GrokProjectionError::UnsupportedSearchRestriction(format!(
                "web_search filters.{key} must contain only strings"
            )));
        };
        projected.push(domain.to_string());
    }
    Ok(Some(projected))
}

fn project_function_tool(tool: &Value) -> GrokTool {
    GrokTool::Function {
        name: json_string(tool, "name").unwrap_or_default(),
        description: json_string(tool, "description"),
        parameters: json_value(tool, "parameters"),
    }
}

fn project_custom_tool(tool: &Value) -> GrokTool {
    GrokTool::Custom {
        name: json_string(tool, "name").unwrap_or_default(),
        description: json_string(tool, "description"),
        format: json_value(tool, "format"),
    }
}

fn x_search_ymd(tool: &Value, key: &str) -> Result<Option<String>, GrokProjectionError> {
    let Some(value) = tool.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(text) = value.as_str() else {
        return Err(GrokProjectionError::UnsupportedSearchRestriction(format!(
            "x_search.{key} must be a calendar YYYY-MM-DD"
        )));
    };
    XSearchProviderConfig::parse_ymd(text)
        .map(Some)
        .ok_or_else(|| {
            GrokProjectionError::UnsupportedSearchRestriction(format!(
                "x_search.{key} `{text}` must be a calendar YYYY-MM-DD"
            ))
        })
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
