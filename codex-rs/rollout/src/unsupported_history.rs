//! Reject retired hosted records before tolerant readers can discard them.

use serde_json::Value;

const RETIRED_X_HISTORY: &str = "unsupported history: this thread contains retired Hosted X Search records; the original history is unchanged";

/// Validate known persisted search shapes without interpreting arbitrary user/tool text.
/// This read-only check follows fixed envelope paths and does not copy any payload.
/// Raw custom-tool responses remain provider-neutral; Grok replay rejects retired
/// hosted X at its own request boundary without changing other providers.
pub fn validate_supported_history(value: &Value) -> std::io::Result<()> {
    let payload = &value["payload"];
    let unsupported = match value["type"].as_str() {
        Some("event_msg") => {
            matches!(
                payload["type"].as_str(),
                Some("item_started" | "item_completed")
            ) && matches!(
                payload["item"]["type"].as_str(),
                Some("XSearch" | "xSearch")
            )
        }
        Some("XSearch" | "xSearch") => true,
        _ => false,
    };
    if unsupported {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            RETIRED_X_HISTORY,
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "unsupported_history_tests.rs"]
mod tests;
