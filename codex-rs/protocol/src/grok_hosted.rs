//! Raw completed Grok hosted shapes, including retired X history.
//! Recognizing a shape is not permission to execute or replay it. Preserve legacy
//! X calls through normalization so the Grok request boundary can reject them
//! honestly instead of synthesizing a local tool result.

use crate::models::ResponseItem;
use crate::models::WebSearchAction;

// A hosted replay item is model-visible context. Keep it below the per-item
// context cap even when the stream allows a larger transport frame.
const MAX_HOSTED_REPLAY_TOKENS: usize = 10_000;

/// Content-free rejection classes for stream diagnostics. The existing replay
/// API still returns its original error text; these classes do not relax it.
#[derive(Debug)]
pub enum HostedReplayError {
    UnsupportedItem,
    MissingIdentity,
    MissingStatus,
    NonCompletedStatus,
    MissingAction,
    UnknownAction,
    ReplayTooLarge,
    InvalidProjection(serde_json::Error),
}

impl HostedReplayError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::UnsupportedItem => "unsupported_item",
            Self::MissingIdentity => "missing_identity",
            Self::MissingStatus => "missing_status",
            Self::NonCompletedStatus => "non_completed_status",
            Self::MissingAction => "missing_action",
            Self::UnknownAction => "unknown_action",
            Self::ReplayTooLarge => "replay_too_large",
            Self::InvalidProjection(_) => "invalid_projection",
        }
    }

    /// An over-cap projection has no exact byte count: the bounded serializer
    /// stopped at the limit. Never serialize it again merely for telemetry.
    pub fn replay_bytes_class(&self) -> &'static str {
        match self {
            Self::ReplayTooLarge => "over_cap",
            Self::InvalidProjection(_) => "within_cap",
            _ => "not_measured",
        }
    }
}

impl std::fmt::Display for HostedReplayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReplayTooLarge => formatter
                .write_str("Grok hosted replay exceeds the 10000 estimated-token context limit"),
            Self::InvalidProjection(error) => std::fmt::Display::fmt(error, formatter),
            _ => formatter.write_str("unsupported Grok hosted replay item"),
        }
    }
}

pub fn hosted_replay_byte_limit() -> usize {
    codex_utils_string::approx_bytes_for_tokens(MAX_HOSTED_REPLAY_TOKENS)
}

pub fn is_x_search_name(name: &str) -> bool {
    matches!(
        name,
        "x_keyword_search" | "x_semantic_search" | "x_user_search" | "x_thread_fetch"
    )
}

pub fn is_completed_x_search(item: &ResponseItem) -> bool {
    matches!(item, ResponseItem::CustomToolCall {
        id: Some(id), status: Some(status), call_id, name, namespace: None, ..
    } if !id.as_str().is_empty() && !call_id.is_empty() && status == "completed"
        && is_x_search_name(name))
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
    project_search_replay_with_diagnostics(item).map_err(|error| error.to_string())
}

/// Same validation and projection as `project_search_replay`, with finite
/// rejection reasons for callers that must report why a completed item failed.
pub fn project_search_replay_with_diagnostics(
    item: &ResponseItem,
) -> Result<serde_json::Value, HostedReplayError> {
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
        _ => return Err(replay_shape_error(item)),
    };
    let json = codex_utils_string::to_json_string_bounded(&view, hosted_replay_byte_limit())
        .map_err(|_| HostedReplayError::ReplayTooLarge)?;
    serde_json::from_str(&json).map_err(HostedReplayError::InvalidProjection)
}

fn replay_shape_error(item: &ResponseItem) -> HostedReplayError {
    let ResponseItem::WebSearchCall {
        id, status, action, ..
    } = item
    else {
        return HostedReplayError::UnsupportedItem;
    };
    if id.as_ref().is_none_or(|id| id.as_str().is_empty()) {
        return HostedReplayError::MissingIdentity;
    }
    match status.as_deref() {
        None => return HostedReplayError::MissingStatus,
        Some("completed") => {}
        Some(_) => return HostedReplayError::NonCompletedStatus,
    }
    match action {
        None => HostedReplayError::MissingAction,
        Some(WebSearchAction::Other) => HostedReplayError::UnknownAction,
        // This helper is reached only after the unchanged admission predicate
        // rejected the item. Keep an honest fallback if that predicate evolves.
        Some(_) => HostedReplayError::UnsupportedItem,
    }
}

#[cfg(test)]
#[path = "grok_hosted_tests.rs"]
mod tests;
