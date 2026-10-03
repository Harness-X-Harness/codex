//! Process-wide ownership of participating filesystem effects, independent of RPC waiters.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;

static MUTATIONS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(/*permits*/ 1)));

/// Closes admission if an operation loses ownership before its effects are known to have settled.
pub(crate) struct EffectGuard {
    owner: Arc<Semaphore>,
    settled: bool,
}

impl EffectGuard {
    pub(crate) fn new() -> Self {
        Self {
            owner: Arc::clone(&MUTATIONS),
            settled: false,
        }
    }

    pub(crate) fn settle(&mut self) {
        self.settled = true;
    }
}

impl Drop for EffectGuard {
    fn drop(&mut self) {
        if !self.settled {
            self.owner.close();
        }
    }
}

struct MutationLease {
    // Fields drop in declaration order: close uncertain admission before releasing the permit.
    effect: EffectGuard,
    _permit: OwnedSemaphorePermit,
}

pub(crate) async fn run<T>(
    operation: impl Future<Output = io::Result<T>> + Send + 'static,
) -> io::Result<T>
where
    T: Send + 'static,
{
    let permit = Arc::clone(&MUTATIONS).acquire_owned().await.map_err(|_| {
        io::Error::other("filesystem mutation admission closed after an uncertain effect")
    })?;
    let lease = MutationLease {
        effect: EffectGuard::new(),
        _permit: permit,
    };
    // No await between admission and ownership transfer. Dropping the JoinHandle detaches;
    // connection cancellation must not abort this owner while a blocking effect survives.
    tokio::spawn(async move {
        // Capture the whole lease, including its permit, rather than only the effect field.
        let mut lease = lease;
        let result = operation.await;
        lease.effect.settle();
        result
    })
    .await
    .map_err(|error| io::Error::other(format!("filesystem mutation task failed: {error}")))?
}

#[cfg(test)]
#[path = "file_system_mutation_test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "file_system_mutation_tests.rs"]
mod tests;
