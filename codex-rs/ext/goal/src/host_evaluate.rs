//! Host-owned round-end evaluation for [`GoalCompletionAuthority::HostEvaluate`].
//!
//! The worker no longer commits `complete` / `blocked` through `update_goal`.
//! After each goal turn stops, the host evaluator returns a verdict and this
//! crate applies it:
//!
//! - `continue` keeps the goal `Active` and stores a next step for idle continuation
//!   when [`crate::GoalHow::AgentTurns`] is set
//! - `candidate_complete` with [`GoalVerification::None`] marks the goal `Complete`
//! - `candidate_complete` with [`GoalVerification::HostSkeptics`] runs the panel;
//!   all not-refuted votes mark `Complete`, any refute stays `Active`, and panel
//!   failure pauses the goal
//! - `blocked` is host-counted; the same `blocker_key` for three consecutive rounds
//!   marks the goal `Blocked`
//! - evaluator failure pauses the goal rather than treating it as complete
//!
//! When no evaluator is installed, HostEvaluate still hides `update_goal` and
//! leaves the goal `Active`. App Server installs
//! [`crate::ModelGoalRoundEvaluator`] and
//! [`crate::GuardianGoalSkepticPanel`] when `goal_host` is on. Turns started
//! with `turn_trigger = "workflow"` skip host evaluation.

use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;

use crate::host_verify::GoalSkepticPanel;
use crate::host_verify::GoalSkepticPanelInput;
use crate::host_verify::apply_skeptic_panel;
use crate::policy::GoalCompletionAuthority;
use crate::policy::GoalVerification;
use crate::runtime::GoalRuntimeHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostGoalStatus {
    Complete,
    Blocked,
    Paused,
}

impl HostGoalStatus {
    pub(crate) fn state(self) -> codex_state::ThreadGoalStatus {
        match self {
            Self::Complete => codex_state::ThreadGoalStatus::Complete,
            Self::Blocked => codex_state::ThreadGoalStatus::Blocked,
            Self::Paused => codex_state::ThreadGoalStatus::Paused,
        }
    }

    pub(crate) fn event_name(self) -> &'static str {
        match self {
            Self::Complete => "host-evaluate-complete",
            Self::Blocked => "host-evaluate-blocked",
            Self::Paused => "host-evaluate-error",
        }
    }
}

/// Consecutive identical host `blocked` verdicts required before the host
/// marks the goal `Blocked`.
pub const HOST_BLOCKED_STREAK_THRESHOLD: u32 = 3;

/// Host evaluator decision for one goal round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEvaluatorDecision {
    Continue,
    CandidateComplete,
    Blocked,
}

/// Structured verdict from a host round-end evaluator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluatorVerdict {
    pub decision: GoalEvaluatorDecision,
    pub evidence: String,
    pub next_step: String,
    pub blocker_key: String,
}

impl GoalEvaluatorVerdict {
    fn validate(self) -> Result<Self, GoalEvaluatorParseError> {
        if self.evidence.trim().is_empty() {
            return Err(GoalEvaluatorParseError::EmptyField("evidence"));
        }
        if self.next_step.trim().is_empty() {
            return Err(GoalEvaluatorParseError::EmptyField("next_step"));
        }
        let key = self.blocker_key.trim();
        match self.decision {
            GoalEvaluatorDecision::Blocked if key.is_empty() => {
                return Err(GoalEvaluatorParseError::EmptyField("blocker_key"));
            }
            GoalEvaluatorDecision::Blocked
                if !key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') =>
            {
                return Err(GoalEvaluatorParseError::InvalidBlockerKey);
            }
            GoalEvaluatorDecision::Continue | GoalEvaluatorDecision::CandidateComplete
                if !key.is_empty() =>
            {
                return Err(GoalEvaluatorParseError::UnexpectedBlockerKey);
            }
            GoalEvaluatorDecision::Continue
            | GoalEvaluatorDecision::CandidateComplete
            | GoalEvaluatorDecision::Blocked => {}
        }
        Ok(Self {
            decision: self.decision,
            evidence: self.evidence,
            next_step: self.next_step,
            blocker_key: key.to_string(),
        })
    }
}

/// Why a host evaluator verdict JSON could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEvaluatorParseError {
    InvalidJson(String),
    EmptyField(&'static str),
    InvalidBlockerKey,
    UnexpectedBlockerKey,
}

impl std::fmt::Display for GoalEvaluatorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(error) => {
                write!(f, "goal evaluator output is not valid JSON: {error}")
            }
            Self::EmptyField(field) => {
                write!(f, "goal evaluator field `{field}` must not be empty")
            }
            Self::InvalidBlockerKey => {
                write!(
                    f,
                    "goal evaluator blocker_key must use lowercase snake_case"
                )
            }
            Self::UnexpectedBlockerKey => write!(
                f,
                "goal evaluator blocker_key must be empty unless decision is blocked"
            ),
        }
    }
}

impl std::error::Error for GoalEvaluatorParseError {}

/// Failure from a host round-end evaluator, including parse failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEvaluatorError {
    Failed(String),
}

impl From<GoalEvaluatorParseError> for GoalEvaluatorError {
    fn from(error: GoalEvaluatorParseError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl std::fmt::Display for GoalEvaluatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GoalEvaluatorError {}

/// Owned input for one host round-end evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalRoundEvaluationInput {
    pub thread_id: codex_protocol::ThreadId,
    pub turn_id: String,
    pub goal_id: String,
    pub objective: String,
}

/// Boxed future returned by [`GoalRoundEvaluator::evaluate`].
pub type GoalRoundEvaluationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<GoalEvaluatorVerdict, GoalEvaluatorError>> + Send + 'a>>;

/// Host-owned completion evaluator invoked after an active goal turn stops.
///
/// Implementations inspect the ending round and return a structured verdict.
/// This crate owns applying that verdict to persisted goal status. Do not
/// spawn worker `spawn_agent` sessions from an implementation. Host skeptics
/// use Guardian [`codex_extension_api::InternalSessionSpawner`] sessions.
pub trait GoalRoundEvaluator: Send + Sync {
    fn evaluate(&self, input: GoalRoundEvaluationInput) -> GoalRoundEvaluationFuture<'_>;
}

/// Parse a host evaluator JSON object into a validated verdict.
pub fn parse_goal_evaluator_verdict(
    raw: &str,
) -> Result<GoalEvaluatorVerdict, GoalEvaluatorParseError> {
    serde_json::from_str::<GoalEvaluatorVerdict>(raw.trim())
        .map_err(|error| GoalEvaluatorParseError::InvalidJson(error.to_string()))?
        .validate()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HostEvaluateRoundState {
    goal_id: Option<String>,
    blocker_key: Option<String>,
    blocked_streak: u32,
    next_step: Option<String>,
}

impl HostEvaluateRoundState {
    pub(crate) fn align_goal(&mut self, goal_id: &str) {
        if self.goal_id.as_deref() == Some(goal_id) {
            return;
        }
        *self = Self {
            goal_id: Some(goal_id.to_string()),
            ..Self::default()
        };
    }

    pub(crate) fn record_blocker(&mut self, blocker_key: &str) -> u32 {
        if self.blocker_key.as_deref() == Some(blocker_key) {
            self.blocked_streak = self.blocked_streak.saturating_add(1);
        } else {
            self.blocker_key = Some(blocker_key.to_string());
            self.blocked_streak = 1;
        }
        self.blocked_streak
    }

    pub(crate) fn reset_blocker(&mut self) {
        self.blocker_key = None;
        self.blocked_streak = 0;
    }

    pub(crate) fn set_next_step(&mut self, next_step: String) {
        self.next_step = Some(next_step);
    }

    pub(crate) fn take_next_step(&mut self) -> Option<String> {
        self.next_step.take()
    }
}

pub(crate) async fn evaluate_active_round(
    runtime: &GoalRuntimeHandle,
    evaluator: Option<&dyn GoalRoundEvaluator>,
    skeptics: Option<&dyn GoalSkepticPanel>,
    turn_id: &str,
) -> Result<(), String> {
    if runtime.policy().completion != GoalCompletionAuthority::HostEvaluate {
        return Ok(());
    }
    let Some(evaluator) = evaluator else {
        return Ok(());
    };
    let Some(goal) = runtime.load_thread_goal().await? else {
        return Ok(());
    };
    if goal.status != codex_state::ThreadGoalStatus::Active {
        return Ok(());
    }

    runtime.align_host_evaluate_goal(&goal.goal_id);
    let verdict = evaluator
        .evaluate(GoalRoundEvaluationInput {
            thread_id: runtime.thread_id(),
            turn_id: turn_id.to_string(),
            goal_id: goal.goal_id,
            objective: goal.objective,
        })
        .await;
    match verdict {
        Ok(verdict) => apply_verdict(runtime, skeptics, turn_id, verdict).await,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "host goal evaluation failed; pausing the goal instead of treating it as complete"
            );
            runtime
                .apply_host_goal_status(turn_id, HostGoalStatus::Paused)
                .await
        }
    }
}

async fn apply_verdict(
    runtime: &GoalRuntimeHandle,
    skeptics: Option<&dyn GoalSkepticPanel>,
    turn_id: &str,
    verdict: GoalEvaluatorVerdict,
) -> Result<(), String> {
    match verdict.decision {
        GoalEvaluatorDecision::Continue => {
            runtime.reset_host_blocker();
            runtime.set_host_next_step(verdict.next_step);
            Ok(())
        }
        GoalEvaluatorDecision::CandidateComplete => {
            runtime.reset_host_blocker();
            match runtime.policy().verification {
                GoalVerification::None => {
                    runtime
                        .apply_host_goal_status(turn_id, HostGoalStatus::Complete)
                        .await
                }
                GoalVerification::HostSkeptics { count } => {
                    let Some(panel) = skeptics else {
                        runtime.set_host_next_step(verdict.next_step);
                        return Ok(());
                    };
                    let Some(goal) = runtime.load_thread_goal().await? else {
                        return Ok(());
                    };
                    apply_skeptic_panel(
                        runtime,
                        turn_id,
                        panel,
                        GoalSkepticPanelInput {
                            thread_id: runtime.thread_id(),
                            turn_id: turn_id.to_string(),
                            goal_id: goal.goal_id,
                            objective: goal.objective,
                            candidate_next_step: verdict.next_step,
                            count,
                        },
                    )
                    .await
                }
            }
        }
        GoalEvaluatorDecision::Blocked => {
            let streak = runtime.record_host_blocker(&verdict.blocker_key);
            if streak >= HOST_BLOCKED_STREAK_THRESHOLD {
                runtime
                    .apply_host_goal_status(turn_id, HostGoalStatus::Blocked)
                    .await
            } else {
                runtime.set_host_next_step(verdict.next_step);
                Ok(())
            }
        }
    }
}
