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
    /// Reserved for a later harness stage. This crate currently still honors
    /// `update_goal` even when a thread is configured with this variant.
    HostEvaluate,
}

/// Whether the host runs an adversarial panel after a completion candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalVerification {
    /// No host verification panel. Current stock behavior.
    None,
    /// Host-owned skeptic sessions.
    ///
    /// `count` is the requested panel size. A later harness stage owns
    /// clamping and spawn.
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
}

impl Default for GoalPolicy {
    fn default() -> Self {
        Self::model_commit()
    }
}
