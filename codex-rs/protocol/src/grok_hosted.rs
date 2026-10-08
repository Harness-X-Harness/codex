//! Supported completed Grok hosted items. Callers must additionally establish
//! the Grok dialect and, at ingress, the request's hosted-tool capability.

use crate::models::ResponseItem;
use crate::models::WebSearchAction;

// A hosted replay item is model-visible context. Keep it below the per-item
// context cap even when the stream allows a larger transport frame.
const MAX_HOSTED_REPLAY_TOKENS: usize = 10_000;

pub fn is_completed_x_search(item: &ResponseItem) -> bool {
    matches!(item, ResponseItem::CustomToolCall {
        id: Some(id), status: Some(status), call_id, name, namespace: None, ..
    } if !id.as_str().is_empty() && !call_id.is_empty() && status == "completed"
        && matches!(name.as_str(), "x_keyword_search" | "x_semantic_search"
            | "x_user_search" | "x_thread_fetch"))
}

pub fn is_completed_web_search(item: &ResponseItem) -> bool {
    let ResponseItem::WebSearchCall {
        id: Some(id),
        status: Some(status),
        action: Some(action),
        ..
    } = item
    else {
        return false;
    };
    if id.as_str().is_empty() || status != "completed" {
        return false;
    }
    match action {
        WebSearchAction::Search { .. }
        | WebSearchAction::OpenPage { .. }
        | WebSearchAction::FindInPage { .. } => true,
        WebSearchAction::Other => false,
    }
}

pub fn is_completed_search(item: &ResponseItem) -> bool {
    is_completed_x_search(item) || is_completed_web_search(item)
}

/// Build the exact outbound view without truncating provider identity or payload.
/// Uses the stock UTF-8-bytes/4 estimate, not a provider tokenizer measurement.
pub fn project_search_replay(item: &ResponseItem) -> Result<serde_json::Value, String> {
    #[derive(serde::Serialize)]
    #[serde(tag = "type")]
    enum Replay<'a> {
        #[serde(rename = "custom_tool_call")]
        X {
            id: &'a crate::ResponseItemId,
            call_id: &'a str,
            name: &'a str,
            input: &'a str,
        },
        #[serde(rename = "web_search_call")]
        Web {
            id: &'a crate::ResponseItemId,
            action: &'a WebSearchAction,
        },
    }
    let view = match item {
        ResponseItem::CustomToolCall {
            id: Some(id),
            call_id,
            name,
            input,
            ..
        } if is_completed_x_search(item) => Replay::X {
            id,
            call_id,
            name,
            input,
        },
        ResponseItem::WebSearchCall {
            id: Some(id),
            action: Some(action),
            ..
        } if is_completed_web_search(item) => Replay::Web { id, action },
        _ => return Err("unsupported Grok hosted replay item".into()),
    };
    let json = codex_utils_string::to_json_string_bounded(
        &view,
        codex_utils_string::approx_bytes_for_tokens(MAX_HOSTED_REPLAY_TOKENS),
    )
    .map_err(|_| {
        "Grok hosted replay exceeds the 10000 estimated-token context limit".to_string()
    })?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "grok_hosted_tests.rs"]
mod tests;
