use std::collections::HashMap;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::grok_hosted_output::GrokHostedOutputOwner;
use codex_protocol::items::TurnItem;

#[derive(Debug)]
pub(crate) enum HostedToolCompletion {
    First { started_item: Option<TurnItem> },
    Duplicate,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HostedToolOwnershipError {
    item_id: String,
    expected: GrokHostedOutputOwner,
    actual: GrokHostedOutputOwner,
    expected_kind: &'static str,
    actual_kind: &'static str,
}

impl std::fmt::Display for HostedToolOwnershipError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let actual = self.actual.label();
        write!(
            formatter,
            "Grok output `{}` changed Tool Plan ownership from {} ({}) to {actual} ({})",
            self.item_id,
            self.expected.label(),
            self.expected_kind,
            self.actual_kind,
        )
    }
}

/// Normalizes provider progress events for one response stream.
///
/// Records the first observed Tool Plan owner before any UI projection or local
/// dispatch. Completed provider items remain the durable truth; this state is
/// request-local lifecycle and UI state only.
#[derive(Debug, Default)]
pub(crate) struct HostedToolLifecycle {
    items: HashMap<String, HostedToolState>,
}

#[derive(Debug)]
struct HostedToolStart {
    item: TurnItem,
    streamed_to_client: bool,
}

#[derive(Debug)]
struct HostedToolState {
    owner: GrokHostedOutputOwner,
    item_kind: &'static str,
    started: Option<HostedToolStart>,
    completed: bool,
}

impl HostedToolLifecycle {
    pub(crate) fn record_added(
        &mut self,
        item_id: &str,
        owner: GrokHostedOutputOwner,
        item_kind: &'static str,
    ) -> Result<bool, HostedToolOwnershipError> {
        if let Some(state) = self.items.get(item_id) {
            self.require_identity(item_id, state.owner, owner, state.item_kind, item_kind)?;
            return Ok(false);
        }
        self.items.insert(
            item_id.to_string(),
            HostedToolState {
                owner,
                item_kind,
                started: None,
                completed: false,
            },
        );
        Ok(true)
    }

    pub(crate) fn attach_started_item(
        &mut self,
        item_id: &str,
        item: TurnItem,
        streamed_to_client: bool,
    ) {
        let state = self
            .items
            .get_mut(item_id)
            .expect("hosted item owner must be recorded before UI projection");
        debug_assert!(!state.completed);
        debug_assert!(state.started.is_none());
        state.started = Some(HostedToolStart {
            item,
            streamed_to_client,
        });
    }

    pub(crate) fn record_completed(
        &mut self,
        item_id: &str,
        owner: GrokHostedOutputOwner,
        item_kind: &'static str,
    ) -> Result<HostedToolCompletion, HostedToolOwnershipError> {
        if let Some(state) = self.items.get_mut(item_id) {
            if state.owner != owner || state.item_kind != item_kind {
                return Err(HostedToolOwnershipError {
                    item_id: item_id.to_string(),
                    expected: state.owner,
                    actual: owner,
                    expected_kind: state.item_kind,
                    actual_kind: item_kind,
                });
            }
            if state.completed {
                return Ok(HostedToolCompletion::Duplicate);
            }
            state.completed = true;
            return Ok(HostedToolCompletion::First {
                started_item: state
                    .started
                    .take()
                    .filter(|start| start.streamed_to_client)
                    .map(|start| start.item),
            });
        }
        self.items.insert(
            item_id.to_string(),
            HostedToolState {
                owner,
                item_kind,
                started: None,
                completed: true,
            },
        );
        Ok(HostedToolCompletion::First { started_item: None })
    }

    fn require_identity(
        &self,
        item_id: &str,
        expected: GrokHostedOutputOwner,
        actual: GrokHostedOutputOwner,
        expected_kind: &'static str,
        actual_kind: &'static str,
    ) -> Result<(), HostedToolOwnershipError> {
        if actual == expected && actual_kind == expected_kind {
            return Ok(());
        }
        Err(HostedToolOwnershipError {
            item_id: item_id.to_string(),
            expected,
            actual,
            expected_kind,
            actual_kind,
        })
    }

    pub(crate) fn take_incomplete(&mut self) -> Vec<TurnItem> {
        self.items
            .drain()
            .filter_map(|(_, mut state)| {
                if state.completed || state.owner == GrokHostedOutputOwner::Ordinary {
                    return None;
                }
                let mut start = state.started.take()?;
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
        let count = self
            .items
            .values()
            .filter(|state| !state.completed && state.owner != GrokHostedOutputOwner::Ordinary)
            .count();
        let incomplete = self.take_incomplete();
        for item in incomplete {
            session.emit_turn_item_completed(turn_context, item).await;
        }
        count
    }
}

#[cfg(test)]
#[path = "hosted_tool_lifecycle_tests.rs"]
mod tests;
