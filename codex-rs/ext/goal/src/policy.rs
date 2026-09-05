//! Completion, verification, and how-work policy for persisted thread goals.
//!
//! These axes are harness concerns. They are independent of Provider
//! selection: a ChatGPT-bound thread and a Grok-bound thread share the same
//! policy type and the same default.
//!
//! Stock behavior is [`GoalPolicy::model_commit`]: the worker commits
//! `complete` or `blocked` through `update_goal`, the host does not run a
//! verifier panel, and the thread pursues the objective with ordinary agent
//! turns plus idle continuation.
//!
//! The stacked host-owned Goal series (round-end evaluation, skeptics,
//! `/workflow`, and optional Goal-to-workflow bind) is one harness feature:
//! `goal_host`. App Server installs [`GoalPolicy::host`] when that flag is on.
//! Later stages extend that constructor; they do not add more feature flags.

/// Who may mark a persisted thread goal `complete` or `blocked`.
///
/// Pause, resume, budget-limit, and usage-limit remain user- or system-owned
/// regardless of this authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalCompletionAuthority {
    /// The worker model commits via `update_goal`. Current stock behavior.
    ModelCommit,
    /// The host evaluates each goal round independently.
    ///
    /// `update_goal` is hidden. After an active goal turn stops, an optional
    /// [`crate::GoalRoundEvaluator`] decides whether the goal stays active,
    /// is complete, or is blocked. Missing evaluators leave the goal active.
    HostEvaluate,
}

/// Default host skeptic panel size when `goal_host` is on.
pub const HOST_SKEPTIC_DEFAULT_COUNT: u8 = 3;
/// Inclusive lower bound for a host skeptic panel.
pub const HOST_SKEPTIC_MIN_COUNT: u8 = 1;
/// Inclusive upper bound for a host skeptic panel.
pub const HOST_SKEPTIC_MAX_COUNT: u8 = 5;

/// Whether the host runs an adversarial panel after a completion candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalVerification {
    /// No host verification panel. Current stock behavior.
    None,
    /// Host-owned skeptic sessions spawned through Guardian internal sessions.
    ///
    /// `count` is the requested panel size. Spawn clamps it to
    /// [`HOST_SKEPTIC_MIN_COUNT`]..=[`HOST_SKEPTIC_MAX_COUNT`].
    HostSkeptics { count: u8 },
}

/// How an active goal makes progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalHow {
    /// Ordinary agent turns plus idle continuation. Current stock behavior.
    AgentTurns,
    /// Bound to a named workflow run.
    ///
    /// Reserved for a later harness stage. This crate currently always
    /// continues through agent turns.
    Workflow,
}

/// Independent completion, verification, and how-work choices for one thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalPolicy {
    pub completion: GoalCompletionAuthority,
    pub verification: GoalVerification,
    pub how: GoalHow,
}

impl GoalPolicy {
    /// Stock Codex Goal: the worker commits completion, with no host panel.
    pub const fn model_commit() -> Self {
        Self {
            completion: GoalCompletionAuthority::ModelCommit,
            verification: GoalVerification::None,
            how: GoalHow::AgentTurns,
        }
    }

    /// Host-owned completion with ordinary agent turns and no verifier panel.
    pub const fn host_evaluate() -> Self {
        Self {
            completion: GoalCompletionAuthority::HostEvaluate,
            verification: GoalVerification::None,
            how: GoalHow::AgentTurns,
        }
    }

    /// Policy installed when the unified `goal_host` harness feature is on.
    ///
    /// Host evaluation plus a Guardian skeptic panel. Later stacked stages
    /// may add an optional workflow bind without adding more feature flags.
    pub const fn host() -> Self {
        Self {
            completion: GoalCompletionAuthority::HostEvaluate,
            verification: GoalVerification::HostSkeptics {
                count: HOST_SKEPTIC_DEFAULT_COUNT,
            },
            how: GoalHow::AgentTurns,
        }
    }
}

impl Default for GoalPolicy {
    fn default() -> Self {
        Self::model_commit()
    }
}
