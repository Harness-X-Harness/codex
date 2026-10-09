//! Bounded, render-only search previews. Canonical items remain the history authority.
//!
//! Attempt identities are monotonically increasing within one live server connection.
//! Core clears an abandoned attempt before retrying, and ingress rejects simultaneously
//! live item-ID collisions, so a canonical item joins only this turn's current attempt.

use super::*;
use crate::history_cell::SearchActivityCell;
use codex_app_server_protocol::SearchActivityNotification;
use codex_app_server_protocol::SearchActivityState;
use std::collections::BTreeMap;

const MAX_PREVIEWS: usize = 65;
const MAX_ITEM_ID_BYTES: usize = 1024;

#[derive(Default)]
pub(super) struct SearchActivityPreviews {
    attempt_id: Option<u64>,
    closed: bool,
    reconciled_through: Option<u64>,
    entries: BTreeMap<u64, SearchPreview>,
}

struct SearchPreview {
    cell: SearchActivityCell,
    completed: bool,
}

impl SearchActivityPreviews {
    pub(super) fn cells(&self) -> impl Iterator<Item = &SearchActivityCell> {
        self.entries.values().map(|entry| &entry.cell)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn observe(&mut self, activity: SearchActivityNotification, animations_enabled: bool) -> bool {
        if self.attempt_id.is_some_and(|id| activity.attempt_id < id) {
            return false;
        }
        let replaced = self.attempt_id != Some(activity.attempt_id) && !self.entries.is_empty();
        if self.attempt_id != Some(activity.attempt_id) {
            *self = Self {
                attempt_id: Some(activity.attempt_id),
                ..Self::default()
            };
        }
        // Cleared is attempt cleanup, including previews that already settled but were
        // never retained canonically. Keep a tombstone until a strictly newer attempt.
        if activity.state == SearchActivityState::Cleared {
            return self.close() || replaced;
        }
        if self.closed
            || self
                .reconciled_through
                .is_some_and(|index| activity.output_index <= index)
        {
            return replaced;
        }
        if let Some(entry) = self.entries.get_mut(&activity.output_index) {
            if entry.cell.item_id() != activity.item_id
                || entry.cell.kind() != activity.kind
                || entry.completed
            {
                return replaced;
            }
            if activity.state == SearchActivityState::Completed {
                entry.completed = true;
                entry.cell.complete();
                return true;
            }
            return replaced;
        }
        if activity.state != SearchActivityState::Running
            || activity.item_id.is_empty()
            || activity.item_id.len() > MAX_ITEM_ID_BYTES
            || self.entries.len() >= MAX_PREVIEWS
            || self.contains(&activity.item_id)
        {
            return replaced;
        }
        self.entries.insert(
            activity.output_index,
            SearchPreview {
                cell: SearchActivityCell::new(activity.item_id, activity.kind, animations_enabled),
                completed: false,
            },
        );
        true
    }

    fn contains(&self, item_id: &str) -> bool {
        self.cells().any(|cell| cell.item_id() == item_id)
    }

    fn reconcile(&mut self, item_id: &str) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .find_map(|(index, entry)| (entry.cell.item_id() == item_id).then_some(*index))
        else {
            return false;
        };
        self.entries.remove(&index);
        // Canonical output is ordered. This watermark prevents late observations from
        // recreating already promoted cells without an unbounded deduplication set.
        self.reconciled_through = Some(index);
        true
    }

    pub(super) fn close(&mut self) -> bool {
        let changed = !self.entries.is_empty();
        self.entries.clear();
        self.closed = true;
        changed
    }
}

impl ChatWidget {
    pub(super) fn on_search_activity(&mut self, activity: SearchActivityNotification) {
        if !self.turn_lifecycle.agent_turn_running
            || self.turn_lifecycle.last_turn_id.as_deref() != Some(activity.turn_id.as_str())
            || self.thread_id.map(|id| id.to_string()).as_deref()
                != Some(activity.thread_id.as_str())
        {
            return;
        }
        let changed = self.transcript.search_activity.observe(
            activity,
            self.local_settings.tui.animations && self.local_settings.tui.effects.progress,
        );
        if changed {
            self.bump_active_cell_revision();
            self.request_redraw();
        }
    }

    pub(super) fn has_search_preview(&self, turn_id: &str, item_id: &str) -> bool {
        self.turn_lifecycle.last_turn_id.as_deref() == Some(turn_id)
            && self.transcript.search_activity.contains(item_id)
    }

    pub(super) fn reconcile_search_preview(&mut self, turn_id: &str, item_id: &str) -> bool {
        if self.turn_lifecycle.last_turn_id.as_deref() != Some(turn_id)
            || !self.transcript.search_activity.reconcile(item_id)
        {
            return false;
        }
        self.bump_active_cell_revision();
        self.request_redraw();
        true
    }

    pub(crate) fn clear_search_activity(&mut self) {
        if self.transcript.search_activity.close() {
            self.bump_active_cell_revision();
            self.request_redraw();
        }
    }
}
