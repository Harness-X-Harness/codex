//! Host skeptic panel invoked after a HostEvaluate `candidate_complete` verdict.
//!
//! Production uses Guardian [`InternalSessionSpawner`] sessions. Tests inject a
//! scripted [`GoalSkepticPanel`]. Missing panels leave the goal `Active`.

use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;

use crate::host_evaluate::HostGoalStatus;
use crate::runtime::GoalRuntimeHandle;

/// Structured vote from one host skeptic.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalSkepticVote {
    pub refuted: bool,
    pub evidence: String,
    pub next_step: String,
}

impl GoalSkepticVote {
    fn validate(self) -> Result<Self, GoalSkepticParseError> {
        if self.evidence.trim().is_empty() {
            return Err(GoalSkepticParseError::EmptyField("evidence"));
        }
        if self.next_step.trim().is_empty() {
            return Err(GoalSkepticParseError::EmptyField("next_step"));
        }
        Ok(Self {
            refuted: self.refuted,
            evidence: self.evidence,
            next_step: self.next_step,
        })
    }
}

/// Why a skeptic JSON object could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalSkepticParseError {
    InvalidJson(String),
    EmptyField(&'static str),
}

impl std::fmt::Display for GoalSkepticParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(error) => {
                write!(f, "goal skeptic output is not valid JSON: {error}")
            }
            Self::EmptyField(field) => {
                write!(f, "goal skeptic field `{field}` must not be empty")
            }
        }
    }
}

impl std::error::Error for GoalSkepticParseError {}

/// Failure from a host skeptic panel, including parse and spawn failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalSkepticError {
    Failed(String),
}

impl From<GoalSkepticParseError> for GoalSkepticError {
    fn from(error: GoalSkepticParseError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl std::fmt::Display for GoalSkepticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GoalSkepticError {}

/// Aggregated panel decision applied to the persisted goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalSkepticPanelVerdict {
    pub refuted: bool,
    pub evidence: String,
    pub next_step: String,
}

/// Owned input for one host skeptic panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalSkepticPanelInput {
    pub thread_id: codex_protocol::ThreadId,
    pub turn_id: String,
    pub goal_id: String,
    pub objective: String,
    pub candidate_next_step: String,
    pub count: u8,
}

/// Boxed future returned by [`GoalSkepticPanel::verify`].
pub type GoalSkepticPanelFuture<'a> =
    Pin<Box<dyn Future<Output = Result<GoalSkepticPanelVerdict, GoalSkepticError>> + Send + 'a>>;

/// Host-owned adversarial panel invoked after a completion candidate.
///
/// Implementations must use Guardian spawners or a test double. Do not spawn
/// worker `spawn_agent` sessions.
pub trait GoalSkepticPanel: Send + Sync {
    fn verify(&self, input: GoalSkepticPanelInput) -> GoalSkepticPanelFuture<'_>;
}

/// Parse one skeptic JSON object into a validated vote.
pub fn parse_goal_skeptic_vote(raw: &str) -> Result<GoalSkepticVote, GoalSkepticParseError> {
    let json = extract_json_object(raw);
    serde_json::from_str::<GoalSkepticVote>(json)
        .map_err(|error| GoalSkepticParseError::InvalidJson(error.to_string()))?
        .validate()
}

/// Combine independent skeptic votes. Any refute keeps the goal active.
pub fn aggregate_skeptic_votes(
    votes: &[GoalSkepticVote],
) -> Result<GoalSkepticPanelVerdict, GoalSkepticError> {
    if votes.is_empty() {
        return Err(GoalSkepticError::Failed(
            "host skeptic panel produced no votes".into(),
        ));
    }
    if let Some(refute) = votes.iter().find(|vote| vote.refuted) {
        return Ok(GoalSkepticPanelVerdict {
            refuted: true,
            evidence: refute.evidence.clone(),
            next_step: refute.next_step.clone(),
        });
    }
    let evidence = votes
        .iter()
        .map(|vote| vote.evidence.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    Ok(GoalSkepticPanelVerdict {
        refuted: false,
        evidence,
        next_step: votes[0].next_step.clone(),
    })
}

pub(crate) async fn apply_skeptic_panel(
    runtime: &GoalRuntimeHandle,
    turn_id: &str,
    panel: &dyn GoalSkepticPanel,
    input: GoalSkepticPanelInput,
) -> Result<(), String> {
    match panel.verify(input).await {
        Ok(verdict) => {
            if verdict.refuted {
                runtime.set_host_next_step(verdict.next_step);
                Ok(())
            } else {
                runtime
                    .apply_host_goal_status(turn_id, HostGoalStatus::Complete)
                    .await
            }
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "host skeptic panel failed; pausing the goal instead of treating it as complete"
            );
            runtime
                .apply_host_goal_status(turn_id, HostGoalStatus::Paused)
                .await
        }
    }
}

fn extract_json_object(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(start) = trimmed.find('{') else {
        return trimmed;
    };
    let Some(end) = trimmed.rfind('}') else {
        return trimmed;
    };
    if end < start {
        return trimmed;
    }
    &trimmed[start..=end]
}
