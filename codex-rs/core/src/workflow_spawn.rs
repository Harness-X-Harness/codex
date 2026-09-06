use std::time::Duration;

use crate::agent::AgentStatus;
use crate::agent::control::SpawnAgentOptions;
use crate::agent::next_thread_spawn_depth;
use crate::codex_thread::CodexThread;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;
use codex_features::Feature;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::user_input::UserInput;

/// Classified stock-spawn wait result. Completed child outcomes stay here so
/// `/workflow` can journal `ok == false` without treating infrastructure
/// failure as a script-visible child error.
#[derive(Debug)]
pub enum StockSpawnWait {
    Completed(String),
    ChildErrored,
    ChildUnavailable,
}

impl CodexThread {
    pub async fn stock_spawn_agent_available(&self) -> bool {
        self.config().await.features.enabled(Feature::MultiAgentV2)
    }

    /// Map a host `/workflow` spawn request onto stock Multi-Agent V2 `spawn_agent`.
    ///
    /// Waits for that existing child turn and returns a classified child outcome.
    /// Does not start a parent-thread turn and does not change stock `spawn_agent`.
    pub async fn spawn_stock_agent_and_wait_text(
        &self,
        message: &str,
        task_name: &str,
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
        loop {
            match self
                .session
                .services
                .agent_control
                .get_status(spawned.thread_id)
                .await
            {
                AgentStatus::Completed(text) => {
                    return Ok(StockSpawnWait::Completed(text.unwrap_or_default()));
                }
                AgentStatus::Errored(_) => return Ok(StockSpawnWait::ChildErrored),
                AgentStatus::Shutdown | AgentStatus::NotFound => {
                    return Ok(StockSpawnWait::ChildUnavailable);
                }
                // Interrupted is not a completed child outcome: the child may
                // still receive more input.
                AgentStatus::PendingInit | AgentStatus::Running | AgentStatus::Interrupted => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
}
