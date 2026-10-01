//! Exhaustive Basic/reasoning whitelist; reject unsupported input before transport.

use crate::common::Reasoning;
use crate::common::ResponsesApiRequest;
use crate::common::TextControls;
use crate::common::TextFormat;
use crate::error::ApiError;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use serde_json::json;

pub(crate) fn build(request: &ResponsesApiRequest) -> Result<Value, ApiError> {
    let ResponsesApiRequest {
        model,
        instructions,
        input,
        tools,
        tool_choice: _,
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
    if let Some(tools) = tools
        && serde_json::from_str::<Value>(tools.as_raw_value().get()).ok() != Some(json!([]))
    {
        return Err(ApiError::Stream(
            "Grok Basic/reasoning does not support nonempty tools".into(),
        ));
    }
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
        ResponseItem::AgentMessage { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCall { .. }
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
