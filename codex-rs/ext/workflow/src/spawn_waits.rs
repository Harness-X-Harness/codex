//! Runtime-only cancellation of a `/workflow` stock-spawn waiter.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_protocol::ThreadId;
use tokio::sync::watch;

pub(crate) struct SpawnWait {
    thread_key: String,
    generation: u64,
    pub(crate) rx: watch::Receiver<bool>,
}

struct SpawnWaitSlot {
    generation: u64,
    tx: watch::Sender<bool>,
}

#[derive(Default)]
pub(crate) struct SpawnWaits {
    next_generation: AtomicU64,
    senders: Mutex<HashMap<String, SpawnWaitSlot>>,
}

impl SpawnWaits {
    pub(crate) fn remember(&self, thread_id: ThreadId) -> SpawnWait {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = watch::channel(false);
        let thread_key = thread_id.to_string();
        let mut senders = self.senders.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = senders.insert(thread_key.clone(), SpawnWaitSlot { generation, tx })
        {
            let _ = previous.tx.send(true);
        }
        SpawnWait {
            thread_key,
            generation,
            rx,
        }
    }

    pub(crate) fn cancel(&self, thread_id: ThreadId) {
        if let Some(slot) = self
            .senders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&thread_id.to_string())
        {
            let _ = slot.tx.send(true);
        }
    }

    pub(crate) fn forget(&self, wait: &SpawnWait) {
        let mut senders = self.senders.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(slot) = senders.get(&wait.thread_key) else {
            return;
        };
        if slot.generation != wait.generation {
            return;
        }
        if let Some(slot) = senders.remove(&wait.thread_key) {
            let _ = slot.tx.send(true);
        }
    }
}

#[cfg(test)]
#[path = "spawn_waits_tests.rs"]
mod tests;
