//! Runtime-only cancellation of a `/workflow` stock-spawn waiter.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_protocol::ThreadId;
use tokio::sync::watch;

#[derive(Default)]
pub(crate) struct SpawnWaits {
    senders: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl SpawnWaits {
    pub(crate) fn remember(&self, thread_id: ThreadId) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        let mut senders = self.senders.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = senders.insert(thread_id.to_string(), tx) {
            let _ = previous.send(true);
        }
        rx
    }

    pub(crate) fn cancel(&self, thread_id: ThreadId) {
        if let Some(tx) = self
            .senders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&thread_id.to_string())
        {
            let _ = tx.send(true);
        }
    }

    pub(crate) fn forget(&self, thread_id: ThreadId) {
        if let Some(tx) = self
            .senders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&thread_id.to_string())
        {
            let _ = tx.send(true);
        }
    }
}

#[cfg(test)]
#[path = "spawn_waits_tests.rs"]
mod tests;
