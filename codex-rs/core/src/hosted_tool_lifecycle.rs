use std::collections::HashMap;
use std::collections::HashSet;

use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseItem;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

#[derive(Debug)]
pub(crate) enum HostedToolCompletion {
    First { started_item: Option<TurnItem> },
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostedToolEventPhase {
    Added,
    Done,
}

/// Normalizes provider progress events for one response stream.
///
/// Only callers that have already established Remote Gateway ownership may add an
/// item. Completed provider items remain the durable truth; this state is UI-only.
#[derive(Debug, Default)]
pub(crate) struct HostedToolLifecycle {
    started: HashMap<String, HostedToolStart>,
    completed: HashSet<String>,
}

#[derive(Debug)]
struct HostedToolStart {
    item: TurnItem,
    streamed_to_client: bool,
}

impl HostedToolLifecycle {
    pub(crate) fn has_seen(&self, item_id: &str) -> bool {
        self.started.contains_key(item_id) || self.completed.contains(item_id)
    }

    pub(crate) fn record_started(
        &mut self,
        item_id: &str,
        item: TurnItem,
        streamed_to_client: bool,
    ) -> bool {
        if self.has_seen(item_id) {
            return false;
        }
        self.started.insert(
            item_id.to_string(),
            HostedToolStart {
                item,
                streamed_to_client,
            },
        );
        true
    }

    pub(crate) fn record_completed(&mut self, item_id: &str) -> HostedToolCompletion {
        if !self.completed.insert(item_id.to_string()) {
            return HostedToolCompletion::Duplicate;
        }
        HostedToolCompletion::First {
            started_item: self
                .started
                .remove(item_id)
                .filter(|start| start.streamed_to_client)
                .map(|start| start.item),
        }
    }

    pub(crate) fn take_incomplete(&mut self) -> Vec<TurnItem> {
        self.started
            .drain()
            .filter_map(|(item_id, mut start)| {
                self.completed.insert(item_id);
                if !start.streamed_to_client {
                    return None;
                }
                if let TurnItem::ImageGeneration(image) = &mut start.item {
                    image.status = "failed".to_string();
                }
                Some(start.item)
            })
            .collect()
    }

    pub(crate) async fn close_incomplete(
        &mut self,
        session: &Session,
        turn_context: &TurnContext,
    ) -> usize {
        let count = self.started.len();
        let incomplete = self.take_incomplete();
        for item in incomplete {
            session.emit_turn_item_completed(turn_context, item).await;
        }
        count
    }
}

pub(crate) fn validate_grok_hosted_item(
    item: &ResponseItem,
    declared_x_search: bool,
    phase: HostedToolEventPhase,
) -> Result<Option<&str>, &'static str> {
    let (id, status, valid_x_call_id) = match item {
        ResponseItem::WebSearchCall { id, status, .. } => (id.as_deref(), status.as_deref(), true),
        ResponseItem::GrokImageGenerationCall { id, status, .. } => {
            (id.as_deref(), Some(status.as_str()), true)
        }
        ResponseItem::CustomToolCall {
            id,
            status,
            call_id,
            ..
        } if declared_x_search => (id.as_deref(), status.as_deref(), !call_id.is_empty()),
        _ => return Ok(None),
    };
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return Err("hosted output is missing provider item_id");
    };
    if !valid_x_call_id {
        return Err("hosted X Search output is missing call_id");
    }
    let valid_status = match phase {
        HostedToolEventPhase::Added => status == Some("in_progress"),
        HostedToolEventPhase::Done => matches!(status, Some("completed" | "failed")),
    };
    if !valid_status {
        return Err(match phase {
            HostedToolEventPhase::Added => "hosted start has an invalid status",
            HostedToolEventPhase::Done => "hosted terminal has an invalid status",
        });
    }
    Ok(Some(id))
}

#[cfg(test)]
#[path = "hosted_tool_lifecycle_tests.rs"]
mod tests;
