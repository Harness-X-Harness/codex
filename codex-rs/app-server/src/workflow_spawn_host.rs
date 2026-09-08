//! App Server adapter from `/workflow` spawn requests onto stock Multi-Agent V2.

use std::sync::Weak;

use codex_core::ChildAgentWait;
use codex_core::ThreadManager;
use codex_core::child_agent_v2_available;
use codex_core::spawn_child_agent_and_wait_text;
use codex_protocol::ThreadId;
use codex_workflow_extension::WorkflowSpawnAvailableFuture;
use codex_workflow_extension::WorkflowSpawnFuture;
use codex_workflow_extension::WorkflowSpawnHost;
use codex_workflow_extension::WorkflowSpawnOutcome;
use codex_workflow_extension::WorkflowSpawnRequest;

pub(crate) struct AppServerWorkflowSpawnHost {
    thread_manager: Weak<ThreadManager>,
}

impl AppServerWorkflowSpawnHost {
    pub(crate) fn new(thread_manager: Weak<ThreadManager>) -> Self {
        Self { thread_manager }
    }
}

impl WorkflowSpawnHost for AppServerWorkflowSpawnHost {
    fn spawn_available(&self, thread_id: ThreadId) -> WorkflowSpawnAvailableFuture<'_> {
        Box::pin(async move {
            let Some(manager) = self.thread_manager.upgrade() else {
                return false;
            };
            let Ok(thread) = manager.get_thread(thread_id).await else {
                return false;
            };
            child_agent_v2_available(&thread).await
        })
    }

    fn spawn_and_wait(
        &self,
        thread_id: ThreadId,
        request: WorkflowSpawnRequest,
    ) -> WorkflowSpawnFuture<'_> {
        Box::pin(async move {
            let Some(manager) = self.thread_manager.upgrade() else {
                return Ok(WorkflowSpawnOutcome::ChildUnavailable);
            };
            let Ok(thread) = manager.get_thread(thread_id).await else {
                return Ok(WorkflowSpawnOutcome::ChildUnavailable);
            };
            if !child_agent_v2_available(&thread).await {
                return Ok(WorkflowSpawnOutcome::ChildUnavailable);
            }
            Ok(
                match spawn_child_agent_and_wait_text(
                    &thread,
                    &request.message,
                    &request.task_name,
                    request.cancel,
                )
                .await?
                {
                    ChildAgentWait::Completed(text) => WorkflowSpawnOutcome::Completed(text),
                    ChildAgentWait::ChildErrored => WorkflowSpawnOutcome::ChildErrored,
                    ChildAgentWait::ChildUnavailable => WorkflowSpawnOutcome::ChildUnavailable,
                    ChildAgentWait::Cancelled => WorkflowSpawnOutcome::Cancelled,
                },
            )
        })
    }
}
