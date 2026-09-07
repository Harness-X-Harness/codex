//! Host evaluator prompt bounding, schema, and retry/parse behavior.
#![allow(clippy::expect_used)]

use codex_goal_extension::EVALUATOR_OUTPUT_MAX_BYTES;
use codex_goal_extension::EVALUATOR_SAMPLE_ATTEMPTS;
use codex_goal_extension::GOAL_VERDICT_BLOCKER_KEY_MAX_CHARS;
use codex_goal_extension::GOAL_VERDICT_EVIDENCE_MAX_CHARS;
use codex_goal_extension::GOAL_VERDICT_NEXT_STEP_MAX_CHARS;
use codex_goal_extension::GoalEvaluatorDecision;
use codex_goal_extension::GoalEvaluatorError;
use codex_goal_extension::GoalEvaluatorParseError;
use codex_goal_extension::build_goal_evaluator_user_payload;
use codex_goal_extension::collect_evaluator_output_text;
use codex_goal_extension::goal_evaluator_evidence;
use codex_goal_extension::goal_evaluator_output_schema;
use codex_goal_extension::parse_goal_evaluator_verdict;
use codex_goal_extension::verdict_from_sample_attempts;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use pretty_assertions::assert_eq;

fn user_message(text: &str) -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

fn assistant_message(text: &str) -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

fn system_message(text: &str) -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Message {
            id: None,
            role: "system".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

fn reasoning_item() -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Reasoning {
            id: None,
            summary: Vec::new(),
            content: None,
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

#[test]
fn evidence_keeps_recent_user_and_assistant_and_skips_system_and_reasoning() {
    let evidence = goal_evaluator_evidence(&[
        system_message("secret system"),
        reasoning_item(),
        user_message("objective"),
        assistant_message("worked"),
        user_message("latest"),
    ]);
    assert!(!evidence.transcript.contains("secret system"));
    assert!(evidence.transcript.contains("[assistant] worked"));
    assert!(evidence.transcript.ends_with("[user] latest"));
    assert_eq!(evidence.plan, None);
}

#[test]
fn evidence_uses_the_latest_plan_update() {
    let first = UpdatePlanArgs {
        explanation: Some("first".into()),
        plan: vec![PlanItemArg {
            step: "old".into(),
            status: StepStatus::Completed,
        }],
    };
    let second = UpdatePlanArgs {
        explanation: None,
        plan: vec![PlanItemArg {
            step: "ship the evaluator".into(),
            status: StepStatus::InProgress,
        }],
    };
    let evidence = goal_evaluator_evidence(&[
        RolloutItem::EventMsg(EventMsg::PlanUpdate(first)),
        assistant_message("still working"),
        RolloutItem::EventMsg(EventMsg::PlanUpdate(second)),
    ]);
    let plan = evidence.plan.expect("plan");
    assert!(plan.contains("[in_progress] ship the evaluator"));
    assert!(!plan.contains("old"));
}

#[test]
fn user_payload_is_json_with_fallback_plan() {
    let payload = build_goal_evaluator_user_payload("ship it", "[user] hi", None);
    let value: serde_json::Value = serde_json::from_str(&payload).expect("json");
    assert_eq!(value["objective"], "ship it");
    assert_eq!(value["transcript"], "[user] hi");
    assert_eq!(value["plan"], "(no plan available)");
}

#[test]
fn output_schema_is_closed_and_requires_verdict_fields() {
    let schema = goal_evaluator_output_schema();
    assert_eq!(schema["additionalProperties"], false);
    let required = schema["required"]
        .as_array()
        .expect("required")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        required,
        vec!["decision", "evidence", "next_step", "blocker_key"]
    );
}

#[tokio::test]
async fn sample_attempts_accept_the_first_valid_verdict() {
    let mut calls = 0usize;
    let verdict = verdict_from_sample_attempts(|_| {
        calls += 1;
        async move {
            Ok(r#"{"decision":"continue","evidence":"tests remain","next_step":"run tests","blocker_key":""}"#.to_string())
        }
    })
    .await
    .expect("verdict");
    assert_eq!(verdict.decision, GoalEvaluatorDecision::Continue);
    assert_eq!(calls, 1);
}

#[tokio::test]
async fn sample_attempts_retry_invalid_json_then_succeed() {
    let mut calls = 0usize;
    let verdict = verdict_from_sample_attempts(|_| {
        calls += 1;
        async move {
            if calls == 1 {
                Ok("not json".to_string())
            } else {
                Ok(r#"{"decision":"candidate_complete","evidence":"deliverable exists","next_step":"stop","blocker_key":""}"#.to_string())
            }
        }
    })
    .await
    .expect("verdict");
    assert_eq!(verdict.decision, GoalEvaluatorDecision::CandidateComplete);
    assert_eq!(calls, EVALUATOR_SAMPLE_ATTEMPTS);
}

#[test]
fn streamed_evaluator_output_rejects_and_clears_oversize() {
    let prefix = r#"{"decision":"continue","evidence":""#;
    let oversize = "x".repeat(EVALUATOR_OUTPUT_MAX_BYTES);
    let error = collect_evaluator_output_text([prefix, oversize.as_str()]).expect_err("cap");
    assert!(!error.to_string().contains('x'));
    assert!(
        collect_evaluator_output_text(["a".repeat(EVALUATOR_OUTPUT_MAX_BYTES)]).is_ok(),
        "bytes at the cap stay accepted"
    );
}

#[test]
fn parse_goal_evaluator_verdict_rejects_oversize_fields() {
    let evidence = "e".repeat(GOAL_VERDICT_EVIDENCE_MAX_CHARS + 1);
    let error = parse_goal_evaluator_verdict(&format!(
        r#"{{"decision":"continue","evidence":"{evidence}","next_step":"run tests","blocker_key":""}}"#
    ))
    .expect_err("evidence cap");
    assert_eq!(
        error,
        GoalEvaluatorParseError::FieldTooLong {
            field: "evidence",
            max_chars: GOAL_VERDICT_EVIDENCE_MAX_CHARS,
        }
    );
    assert!(!error.to_string().contains(&evidence));

    let evidence = "e".repeat(GOAL_VERDICT_EVIDENCE_MAX_CHARS);
    parse_goal_evaluator_verdict(&format!(
        r#"{{"decision":"continue","evidence":"{evidence}","next_step":"run tests","blocker_key":""}}"#
    ))
    .expect("evidence at cap");

    let next_step = "n".repeat(GOAL_VERDICT_NEXT_STEP_MAX_CHARS + 1);
    let error = parse_goal_evaluator_verdict(&format!(
        r#"{{"decision":"continue","evidence":"tests remain","next_step":"{next_step}","blocker_key":""}}"#
    ))
    .expect_err("next_step cap");
    assert_eq!(
        error,
        GoalEvaluatorParseError::FieldTooLong {
            field: "next_step",
            max_chars: GOAL_VERDICT_NEXT_STEP_MAX_CHARS,
        }
    );

    let blocker_key = format!("k{}", "a".repeat(GOAL_VERDICT_BLOCKER_KEY_MAX_CHARS));
    let error = parse_goal_evaluator_verdict(&format!(
        r#"{{"decision":"blocked","evidence":"need access","next_step":"ask the user","blocker_key":"{blocker_key}"}}"#
    ))
    .expect_err("blocker_key cap");
    assert_eq!(
        error,
        GoalEvaluatorParseError::FieldTooLong {
            field: "blocker_key",
            max_chars: GOAL_VERDICT_BLOCKER_KEY_MAX_CHARS,
        }
    );
}

#[tokio::test]
async fn sample_attempts_fail_closed_after_retries() {
    let error = verdict_from_sample_attempts(|_| async {
        Err(GoalEvaluatorError::Failed("upstream down".into()))
    })
    .await
    .expect_err("retries exhausted");
    assert_eq!(error.to_string(), "upstream down");
}
