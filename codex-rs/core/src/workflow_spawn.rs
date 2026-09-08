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

/// Classified stock Multi-Agent V2 wait result for a host adapter.
///
/// This is not the `/workflow` script contract. The Workflow extension owns
/// the classified spawn outcome type.
#[derive(Debug, PartialEq, Eq)]
pub enum ChildAgentWait {
    Completed(String),
    ChildErrored,
    ChildUnavailable,
    Cancelled,
}

pub async fn child_agent_v2_available(thread: &CodexThread) -> bool {
    thread
        .config()
        .await
        .features
        .enabled(Feature::MultiAgentV2)
}

/// Map a host spawn request onto stock Multi-Agent V2 `spawn_agent`.
///
/// Waits for that existing child turn and returns a classified child outcome.
/// Does not start a parent-thread turn and does not change stock `spawn_agent`.
/// `cancel` becoming `true` drops this wait without interrupting the stock child.
pub async fn spawn_child_agent_and_wait_text(
    thread: &CodexThread,
    message: &str,
    task_name: &str,
    cancel: watch::Receiver<bool>,
) -> CodexResult<ChildAgentWait> {
    if !child_agent_v2_available(thread).await {
        return Err(CodexErr::InvalidRequest(
            "stock spawn_agent is unavailable".to_string(),
        ));
    }
    let config = thread.config().await;
    let session_source = thread_spawn_source(
        thread.session.thread_id,
        &thread.session_source,
        next_thread_spawn_depth(&thread.session_source),
        /*agent_role*/ None,
        Some(task_name.to_string()),
    )
    .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
    let spawned = thread
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
                parent_thread_id: Some(thread.session.thread_id),
                ..Default::default()
            },
        )
        .await?;
    if *cancel.borrow() {
        return Ok(ChildAgentWait::Cancelled);
    }
    let child_id = spawned.thread_id;
    let status_rx = match thread
        .session
        .services
        .agent_control
        .subscribe_status(child_id)
        .await
    {
        Ok(status_rx) => status_rx,
        Err(_) => {
            let status = thread
                .session
                .services
                .agent_control
                .get_status(child_id)
                .await;
            return Ok(classify_stock_status(status).unwrap_or(ChildAgentWait::Cancelled));
        }
    };
    Ok(wait_classified_stock_status(status_rx, cancel, || {
        thread.session.services.agent_control.get_status(child_id)
    })
    .await)
}

fn classify_stock_status(status: AgentStatus) -> Option<ChildAgentWait> {
    match status {
        AgentStatus::Completed(text) => Some(ChildAgentWait::Completed(text.unwrap_or_default())),
        AgentStatus::Errored(_) => Some(ChildAgentWait::ChildErrored),
        AgentStatus::Shutdown | AgentStatus::NotFound => Some(ChildAgentWait::ChildUnavailable),
        AgentStatus::PendingInit | AgentStatus::Running | AgentStatus::Interrupted => None,
    }
}

async fn wait_classified_stock_status<F, Fut>(
    mut status_rx: watch::Receiver<AgentStatus>,
    mut cancel: watch::Receiver<bool>,
    mut refresh_status: F,
) -> ChildAgentWait
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AgentStatus>,
{
    loop {
        if *cancel.borrow() {
            return ChildAgentWait::Cancelled;
        }
        if let Some(done) = classify_stock_status(status_rx.borrow().clone()) {
            return done;
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return ChildAgentWait::Cancelled;
                }
            }
            changed = status_rx.changed() => {
                if changed.is_err() {
                    return classify_stock_status(refresh_status().await)
                        .unwrap_or(ChildAgentWait::Cancelled);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "workflow_spawn_tests.rs"]
mod tests;
