//! Runtime-only ownership of a started same-Thread Workflow host turn.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_protocol::ThreadId;

#[derive(Default)]
pub(crate) struct InFlightTurns {
    turns: Mutex<HashMap<String, String>>,
}

impl InFlightTurns {
    pub(crate) fn remember(&self, thread_id: ThreadId, turn_id: String) {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(thread_id.to_string(), turn_id);
    }

    pub(crate) fn forget(&self, thread_id: ThreadId) {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&thread_id.to_string());
    }

    pub(crate) fn owns(&self, thread_id: ThreadId, turn_id: Option<&str>) -> bool {
        let owned = self
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&thread_id.to_string())
            .cloned();
        match (owned.as_deref(), turn_id) {
            (None, _) => true,
            (Some(owned), Some(turn_id)) => owned == turn_id,
            (Some(_), None) => false,
        }
    }
}

#[cfg(test)]
#[path = "inflight_tests.rs"]
mod tests;
