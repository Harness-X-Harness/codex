#![allow(clippy::expect_used)]

//! One-shot, path-scoped scheduling controls for real filesystem effects.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::LazyLock;
use std::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::oneshot;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum Phase {
    Waiting,
    Compared,
    Blocking,
    Settled,
}

type Key = (PathBuf, Phase);
static GATES: LazyLock<Mutex<HashMap<Key, Arc<Gate>>>> = LazyLock::new(Mutex::default);

struct Gate {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    released: Mutex<bool>,
    blocking: Condvar,
    asynchronous: Notify,
}

impl Gate {
    fn enter(&self) {
        if let Some(entered) = self.entered.lock().expect("gate entry").take() {
            let _ = entered.send(());
        }
    }

    fn release(&self) {
        *self.released.lock().expect("gate release") = true;
        self.blocking.notify_all();
        self.asynchronous.notify_one();
    }
}

pub(crate) struct Pause {
    key: Key,
    gate: Arc<Gate>,
    entered: oneshot::Receiver<()>,
}

impl Pause {
    pub(crate) fn new(path: &Path, phase: Phase) -> Self {
        let (entered, receiver) = oneshot::channel();
        let gate = Arc::new(Gate {
            entered: Mutex::new(Some(entered)),
            released: Mutex::new(false),
            blocking: Condvar::new(),
            asynchronous: Notify::new(),
        });
        let native = codex_utils_path_uri::PathUri::from_host_native_path(path)
            .expect("gate path URI")
            .to_abs_path()
            .expect("gate native path");
        let key = (native.into_path_buf(), phase);
        assert!(
            GATES
                .lock()
                .expect("gate registry")
                .insert(key.clone(), Arc::clone(&gate))
                .is_none()
        );
        Self {
            key,
            gate,
            entered: receiver,
        }
    }

    pub(crate) async fn entered(&mut self) {
        tokio::time::timeout(
            std::time::Duration::from_secs(/*secs*/ 10),
            &mut self.entered,
        )
        .await
        .expect("effect reached barrier")
        .expect("effect entry signal");
    }

    pub(crate) fn release(&self) {
        self.gate.release();
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        self.gate.release();
        GATES.lock().expect("gate registry").remove(&self.key);
    }
}

pub(crate) async fn pause(path: &Path, phase: Phase) {
    let gate = GATES
        .lock()
        .expect("gate registry")
        .remove(&(path.to_path_buf(), phase));
    if let Some(gate) = gate {
        gate.enter();
        loop {
            let released = *gate.released.lock().expect("gate state");
            if released {
                break;
            }
            gate.asynchronous.notified().await;
        }
    }
}

pub(crate) fn pause_blocking(path: &Path) {
    let gate = GATES
        .lock()
        .expect("gate registry")
        .remove(&(path.to_path_buf(), Phase::Blocking));
    if let Some(gate) = gate {
        gate.enter();
        let mut released = gate.released.lock().expect("gate state");
        while !*released {
            released = gate.blocking.wait(released).expect("blocking gate");
        }
    }
}

pub(crate) fn available_permits() -> usize {
    super::MUTATIONS.available_permits()
}

/// Observe a real pending admission future without delaying or replacing its polling.
pub(crate) async fn observe_pending<T>(
    path: &codex_utils_path_uri::PathUri,
    future: impl Future<Output = T>,
) -> T {
    let native = path.to_abs_path().ok().map(|path| path.into_path_buf());
    tokio::pin!(future);
    std::future::poll_fn(|context| {
        let result = future.as_mut().poll(context);
        if result.is_pending()
            && let Some(gate) = native.as_ref().and_then(|path| {
                GATES
                    .lock()
                    .expect("gate registry")
                    .remove(&(path.clone(), Phase::Waiting))
            })
        {
            gate.enter();
        }
        result
    })
    .await
}
