//! Exhaustive Grok whitelist; reject unsupported input before transport.

use crate::common::Reasoning;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::common::TextControls;
use crate::common::TextFormat;
use crate::error::ApiError;
use crate::grok_search::project_web_search;
use crate::grok_search::project_x_search;
use codex_protocol::grok::GrokXSearchOptions;
use codex_protocol::grok_hosted::is_completed_search;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use serde_json::Value;
use serde_json::json;

pub(crate) fn build_with_search(
    request: &ResponsesApiRequest,
    x_search: Option<&GrokXSearchOptions>,
) -> Result<Value, ApiError> {
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
    let input = input
        .iter()
        .enumerate()
        .map(|(index, item)| project_item(index, item))
        .collect::<Result<Vec<_>, _>>()?;
    let reasoning = reasoning.as_ref().map(
        |Reasoning {
             effort,
             summary,
             context: _,
         }| Reasoning {
            effort: effort.clone(),
            summary: *summary,
            context: None,
        },
    );
    let mut body = json!({
        "model": model, "input": input, "reasoning": reasoning,
        "stream": stream, "include": include,
    });
    if let Some(tools) = tools {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!(tool_choice);
    }
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions);
    }
    if let Some(key) = prompt_cache_key {
        body["prompt_cache_key"] = json!(key);
    }
    if let Some(TextControls {
        verbosity: _,
        format: Some(format),
    }) = text
    {
        let TextFormat {
            r#type: _,
            strict: _,
            schema: _,
            name: _,
        } = format;
        body["text"] = json!({"format": format});
    }
    Ok(body)
}

fn project_tools(
    tools: Option<&ResponsesApiTools>,
    x_search: Option<&GrokXSearchOptions>,
) -> Result<Option<Vec<Value>>, ApiError> {
    if let Some(defaults) = x_search {
        defaults.validate().map_err(ApiError::Stream)?;
    }
    let Some(tools) = tools else {
        return Ok(None);
    };
    let tools: Vec<Value> = serde_json::from_str(tools.as_raw_value().get())
        .map_err(|_| ApiError::Stream("Grok requires a tool array".into()))?;
    let mut projected = Vec::with_capacity(tools.len() + 1);
    let mut has_x_search = false;
    for (index, tool) in tools.into_iter().enumerate() {
        let Some(fields) = tool.as_object() else {
            return Err(ApiError::Stream(format!(
                "Grok requires a tool object at tools[{index}]"
            )));
        };
        match fields.get("type").and_then(Value::as_str) {
            Some("web_search") => {
                projected.push(project_web_search(fields, index)?);
                continue;
            }
            Some("x_search") => {
                if has_x_search {
                    return Err(ApiError::Stream(
                        "Grok requires at most one x_search tool".into(),
                    ));
                }
                has_x_search = true;
                projected.push(project_x_search(fields, x_search)?);
                continue;
            }
            _ => {}
        }
        if fields.get("type").and_then(Value::as_str) != Some("function")
            || fields
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || fields.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "type" | "name" | "description" | "parameters" | "strict" | "defer_loading"
                )
            })
        {
            return Err(ApiError::Stream(format!(
                "Grok requires a projected local function at tools[{index}]"
            )));
        }
        let mut function = json!({"type": "function", "name": fields["name"]});
        for key in ["description", "parameters"] {
            if let Some(value) = fields.get(key).filter(|value| !value.is_null()) {
                if (key == "description" && !value.is_string())
                    || (key == "parameters" && !value.is_object())
                {
                    return Err(ApiError::Stream(format!(
                        "Grok cannot project tools[{index}].{key}"
                    )));
                }
                function[key] = value.clone();
            }
        }
        projected.push(function);
    }
    if !projected.is_empty() && !has_x_search {
        projected.push(project_x_search(&serde_json::Map::new(), x_search)?);
    }
    Ok((!projected.is_empty()).then_some(projected))
}

fn project_item(index: usize, item: &ResponseItem) -> Result<Value, ApiError> {
    let mut projected = match item {
        ResponseItem::Message {
            id: _,
            role,
            content,
            phase: _,
            internal_chat_message_metadata_passthrough: _,
        } => {
            let parts = content
                .iter()
                .map(|part| match part {
                    ContentItem::InputText { text: _ } | ContentItem::OutputText { text: _ } => {
                        Ok(json!(part))
                    }
                    ContentItem::InputImage {
                        image: _,
                        detail: _,
                    }
                    | ContentItem::InputAudio { audio_url: _ } => Err(ApiError::Stream(format!(
                        "Grok Basic/reasoning does not support media at input[{index}]"
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            json!({"type": "message", "role": role, "content": parts})
        }
        ResponseItem::Reasoning {
            id: _,
            summary,
            content,
            encrypted_content,
            internal_chat_message_metadata_passthrough: _,
        } => {
            let summary = summary
                .iter()
                .map(|part| match part {
                    ReasoningItemReasoningSummary::SummaryText { text: _ } => json!(part),
                })
                .collect::<Vec<_>>();
            let mut reasoning = json!({"type": "reasoning", "summary": summary});
            if let Some(blob) = encrypted_content {
                // Even Some("") is authoritative: never replay plaintext alongside it.
                reasoning["encrypted_content"] = json!(blob);
            } else if let Some(content) = content {
                let content = content
                    .iter()
                    .map(|part| match part {
                        ReasoningItemContent::ReasoningText { text: _ } => Ok(json!(part)),
                        ReasoningItemContent::Text { text: _ } => Err(ApiError::Stream(format!(
                            "Grok cannot replay untyped reasoning at input[{index}]"
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                reasoning["content"] = json!(content);
            }
            reasoning
        }
        ResponseItem::FunctionCall {
            id: _,
            name,
            namespace,
            arguments,
            encrypted_function_args,
            call_id,
            internal_chat_message_metadata_passthrough: _,
        } => {
            if namespace.is_some() || encrypted_function_args.is_some() {
                return Err(ApiError::Stream(format!(
                    "Grok requires a projected plaintext function call at input[{index}]"
                )));
            }
            json!({"type": "function_call", "name": name,
                "arguments": arguments, "call_id": call_id})
        }
        ResponseItem::FunctionCallOutput {
            id: _,
            call_id,
            name,
            namespace,
            output,
            internal_chat_message_metadata_passthrough: _,
        } => {
            let Some(call_id) = call_id.as_deref().filter(|id| !id.is_empty()) else {
                return Err(ApiError::Stream(format!(
                    "Grok requires function output call_id at input[{index}]"
                )));
            };
            if namespace.is_some() {
                return Err(ApiError::Stream(format!(
                    "Grok requires a projected function output at input[{index}]"
                )));
            }
            let output = match &output.body {
                FunctionCallOutputBody::Text(text) => json!(text),
                FunctionCallOutputBody::ContentItems(parts) => {
                    let parts = parts
                        .iter()
                        .map(|part| match part {
                            FunctionCallOutputContentItem::InputText { text: _ }
                            | FunctionCallOutputContentItem::InputImage {
                                image: ImageReference::Inline { image_url: _ },
                                detail: _,
                            } => Ok(json!(part)),
                            FunctionCallOutputContentItem::InputImage {
                                image: ImageReference::File { file_id: _ },
                                detail: _,
                            }
                            | FunctionCallOutputContentItem::InputAudio { audio_url: _ }
                            | FunctionCallOutputContentItem::EncryptedContent {
                                encrypted_content: _,
                            } => Err(ApiError::Stream(format!(
                                "Grok cannot replay this function output content at input[{index}]"
                            ))),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    json!(parts)
                }
            };
            let mut item = json!({"type": "function_call_output", "call_id": call_id,
                "output": output});
            if let Some(name) = name {
                item["name"] = json!(name);
            }
            item
        }
        ResponseItem::WebSearchCall { .. } | ResponseItem::CustomToolCall { .. }
            if is_completed_search(item) =>
        {
            codex_protocol::grok_hosted::project_search_replay(item).map_err(ApiError::Stream)?
        }
        ResponseItem::AgentMessage {
            content,
            id: _,
            author: _,
            recipient: _,
            internal_chat_message_metadata_passthrough: _,
        } => {
            let text = plaintext_agent_message_content(content).ok_or_else(|| {
                ApiError::Stream(format!(
                    "Grok cannot replay empty or encrypted collaboration history at input[{index}]"
                ))
            })?;
            // Stock already rendered the author, recipient and message kind in this
            // plaintext envelope. Project only the wire copy; retain canonical history.
            json!({"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": text}]})
        }
        ResponseItem::CustomToolCall { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::ConfigurationUpdate { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::AdditionalTools { .. }
        | ResponseItem::CompactionTrigger {}
        | ResponseItem::Other => {
            return Err(ApiError::Stream(format!(
                "Grok Basic/reasoning cannot replay this history at input[{index}]"
            )));
        }
    };
    if let Some(id) = item.id() {
        projected["id"] = json!(id);
    }
    Ok(projected)
}

#[cfg(test)]
fn build(request: &ResponsesApiRequest) -> Result<Value, ApiError> {
    build_with_search(request, /*x_search*/ None)
}

#[cfg(test)]
#[path = "grok_request_tests.rs"]
mod tests;
