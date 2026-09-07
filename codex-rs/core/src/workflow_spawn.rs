use crate::agent::AgentStatus;
use crate::agent::control::SpawnAgentOptions;
use crate::agent::next_thread_spawn_depth;
use crate::codex_thread::CodexThread;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;
use codex_features::Feature;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::user_input::UserInput;
use tokio::sync::watch;

/// Classified stock-spawn wait result. Completed child outcomes stay here so
/// `/workflow` can journal `ok == false` without treating infrastructure
/// failure as a script-visible child error.
#[derive(Debug, PartialEq, Eq)]
pub enum StockSpawnWait {
    Completed(String),
    ChildErrored,
    ChildUnavailable,
    Cancelled,
}

impl CodexThread {
    pub async fn stock_spawn_agent_available(&self) -> bool {
        self.config().await.features.enabled(Feature::MultiAgentV2)
    }

    /// Map a host `/workflow` spawn request onto stock Multi-Agent V2 `spawn_agent`.
    ///
    /// Waits for that existing child turn and returns a classified child outcome.
    /// Does not start a parent-thread turn and does not change stock `spawn_agent`.
    /// `cancel` becoming `true` drops this wait without interrupting the stock child.
    pub async fn spawn_stock_agent_and_wait_text(
        &self,
        message: &str,
        task_name: &str,
        cancel: watch::Receiver<bool>,
    ) -> CodexResult<StockSpawnWait> {
        if !self.stock_spawn_agent_available().await {
            return Err(CodexErr::InvalidRequest(
                "stock spawn_agent is unavailable".to_string(),
            ));
        }
        let config = self.config().await;
        let session_source = thread_spawn_source(
            self.session.thread_id,
            &self.session_source,
            next_thread_spawn_depth(&self.session_source),
            /*agent_role*/ None,
            Some(task_name.to_string()),
        )
        .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let spawned = self
            .session
            .services
            .agent_control
            .spawn_agent_with_metadata(
                (*config).clone(),
                vec![UserInput::Text {
                    text: message.to_string(),
                    text_elements: Vec::new(),
                }],
                Some(session_source),
                SpawnAgentOptions {
                    parent_thread_id: Some(self.session.thread_id),
                    ..Default::default()
                },
            )
            .await?;
        if *cancel.borrow() {
            return Ok(StockSpawnWait::Cancelled);
        }
        let child_id = spawned.thread_id;
        let status_rx = match self
            .session
            .services
            .agent_control
            .subscribe_status(child_id)
            .await
        {
            Ok(status_rx) => status_rx,
            Err(_) => {
                let status = self
                    .session
                    .services
                    .agent_control
                    .get_status(child_id)
                    .await;
                return Ok(classify_stock_status(status).unwrap_or(StockSpawnWait::Cancelled));
            }
        };
        Ok(wait_classified_stock_status(status_rx, cancel, || {
            self.session.services.agent_control.get_status(child_id)
        })
        .await)
    }
}

fn classify_stock_status(status: AgentStatus) -> Option<StockSpawnWait> {
    match status {
        AgentStatus::Completed(text) => Some(StockSpawnWait::Completed(text.unwrap_or_default())),
        AgentStatus::Errored(_) => Some(StockSpawnWait::ChildErrored),
        AgentStatus::Shutdown | AgentStatus::NotFound => Some(StockSpawnWait::ChildUnavailable),
        AgentStatus::PendingInit | AgentStatus::Running | AgentStatus::Interrupted => None,
    }
}

async fn wait_classified_stock_status<F, Fut>(
    mut status_rx: watch::Receiver<AgentStatus>,
    mut cancel: watch::Receiver<bool>,
    mut refresh_status: F,
) -> StockSpawnWait
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AgentStatus>,
{
    loop {
        if *cancel.borrow() {
            return StockSpawnWait::Cancelled;
        }
        if let Some(done) = classify_stock_status(status_rx.borrow().clone()) {
            return done;
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return StockSpawnWait::Cancelled;
                }
            }
            changed = status_rx.changed() => {
                if changed.is_err() {
                    return classify_stock_status(refresh_status().await)
                        .unwrap_or(StockSpawnWait::Cancelled);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "workflow_spawn_tests.rs"]
mod tests;
