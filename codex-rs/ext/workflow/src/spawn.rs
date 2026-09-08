//! Constructor-injected stock-spawn host for `/workflow`.
//!
//! The extension owns the request and classified outcome. The host maps that
//! contract onto stock Multi-Agent V2. Do not expose AgentControl to Rhai.

use std::future::Future;
use std::pin::Pin;

use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use tokio::sync::watch;

/// One `/workflow` stock-spawn request.
pub struct WorkflowSpawnRequest {
    pub message: String,
    pub task_name: String,
    pub cancel: watch::Receiver<bool>,
}

/// Classified stock-spawn wait result owned by `/workflow`.
///
/// Completed child outcomes stay here so the script can journal `ok == false`
/// without treating infrastructure failure as a child error.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkflowSpawnOutcome {
    Completed(String),
    ChildErrored,
    ChildUnavailable,
    Cancelled,
}

/// Boxed future returned by [`WorkflowSpawnHost::spawn_available`].
pub type WorkflowSpawnAvailableFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Boxed future returned by [`WorkflowSpawnHost::spawn_and_wait`].
pub type WorkflowSpawnFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkflowSpawnOutcome, CodexErr>> + Send + 'a>>;

/// Host maps a Workflow spawn request onto stock Multi-Agent V2.
///
/// Implementations must preserve event-driven status waiting, Workflow-side
/// cancellation, and the classified child outcomes. They must not create a
/// second engine occupant or a new child runtime.
pub trait WorkflowSpawnHost: Send + Sync {
    fn spawn_available(&self, thread_id: ThreadId) -> WorkflowSpawnAvailableFuture<'_>;

    fn spawn_and_wait(
        &self,
        thread_id: ThreadId,
        request: WorkflowSpawnRequest,
    ) -> WorkflowSpawnFuture<'_>;
}
