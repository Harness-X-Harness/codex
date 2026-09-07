//! Host skeptic vote parsing, clamp, and aggregation.

use codex_goal_extension::GOAL_VERDICT_EVIDENCE_MAX_CHARS;
use codex_goal_extension::GOAL_VERDICT_NEXT_STEP_MAX_CHARS;
use codex_goal_extension::GoalCompletionAuthority;
use codex_goal_extension::GoalPolicy;
use codex_goal_extension::GoalSkepticPanelVerdict;
use codex_goal_extension::GoalSkepticParseError;
use codex_goal_extension::GoalSkepticVote;
use codex_goal_extension::GoalVerification;
use codex_goal_extension::HOST_SKEPTIC_DEFAULT_COUNT;
use codex_goal_extension::HOST_SKEPTIC_MAX_COUNT;
use codex_goal_extension::HOST_SKEPTIC_MIN_COUNT;
use codex_goal_extension::aggregate_skeptic_votes;
use codex_goal_extension::clamp_host_skeptic_count;
use codex_goal_extension::parse_goal_skeptic_vote;
use pretty_assertions::assert_eq;

#[test]
fn host_policy_installs_default_skeptic_panel() {
    assert_eq!(
        GoalPolicy::host(),
        GoalPolicy {
            completion: GoalCompletionAuthority::HostEvaluate,
            verification: GoalVerification::HostSkeptics {
                count: HOST_SKEPTIC_DEFAULT_COUNT
            },
        }
    );
}

#[test]
fn clamp_host_skeptic_count_bounds_panel_size() {
    assert_eq!(clamp_host_skeptic_count(0), HOST_SKEPTIC_MIN_COUNT);
    assert_eq!(clamp_host_skeptic_count(3), 3);
    assert_eq!(
        clamp_host_skeptic_count(HOST_SKEPTIC_MAX_COUNT.saturating_add(4)),
        HOST_SKEPTIC_MAX_COUNT
    );
}

#[test]
fn parse_goal_skeptic_vote_accepts_object_inside_fences() {
    let vote = parse_goal_skeptic_vote(
        "```json\n{\"refuted\":true,\"evidence\":\"missing proof\",\"next_step\":\"add tests\"}\n```",
    )
    .expect("vote should parse");
    assert_eq!(
        vote,
        GoalSkepticVote {
            refuted: true,
            evidence: "missing proof".into(),
            next_step: "add tests".into(),
        }
    );
}

#[test]
fn parse_goal_skeptic_vote_rejects_oversize_evidence() {
    let evidence = "e".repeat(GOAL_VERDICT_EVIDENCE_MAX_CHARS + 1);
    let error = parse_goal_skeptic_vote(&format!(
        r#"{{"refuted":false,"evidence":"{evidence}","next_step":"none"}}"#
    ))
    .expect_err("evidence cap");
    assert_eq!(
        error,
        GoalSkepticParseError::FieldTooLong {
            field: "evidence",
            max_chars: GOAL_VERDICT_EVIDENCE_MAX_CHARS,
        }
    );
    assert!(!error.to_string().contains(&evidence));
}

#[test]
fn parse_goal_skeptic_vote_rejects_oversize_next_step() {
    let next_step = "n".repeat(GOAL_VERDICT_NEXT_STEP_MAX_CHARS + 1);
    let error = parse_goal_skeptic_vote(&format!(
        r#"{{"refuted":true,"evidence":"missing proof","next_step":"{next_step}"}}"#
    ))
    .expect_err("next_step cap");
    assert_eq!(
        error,
        GoalSkepticParseError::FieldTooLong {
            field: "next_step",
            max_chars: GOAL_VERDICT_NEXT_STEP_MAX_CHARS,
        }
    );
}

#[test]
fn parse_goal_skeptic_vote_rejects_empty_evidence() {
    assert_eq!(
        parse_goal_skeptic_vote(r#"{"refuted":false,"evidence":"","next_step":"none"}"#),
        Err(GoalSkepticParseError::EmptyField("evidence"))
    );
}

#[test]
fn aggregate_skeptic_votes_any_refute_keeps_that_next_step() {
    let votes = [
        GoalSkepticVote {
            refuted: false,
            evidence: "tests pass".into(),
            next_step: "none".into(),
        },
        GoalSkepticVote {
            refuted: true,
            evidence: "artifact missing".into(),
            next_step: "restore the artifact".into(),
        },
    ];
    let verdict = aggregate_skeptic_votes(&votes).expect("votes should aggregate");
    assert_eq!(
        verdict,
        GoalSkepticPanelVerdict {
            refuted: true,
            evidence: "artifact missing".into(),
            next_step: "restore the artifact".into(),
        }
    );
}

#[test]
fn aggregate_skeptic_votes_all_clear_confirms() {
    let votes = [
        GoalSkepticVote {
            refuted: false,
            evidence: "tests pass".into(),
            next_step: "none".into(),
        },
        GoalSkepticVote {
            refuted: false,
            evidence: "artifact present".into(),
            next_step: "none".into(),
        },
    ];
    let verdict = aggregate_skeptic_votes(&votes).expect("votes should aggregate");
    assert_eq!(
        verdict,
        GoalSkepticPanelVerdict {
            refuted: false,
            evidence: "tests pass; artifact present".into(),
            next_step: "none".into(),
        }
    );
}
