//! Prompt construction and verdict retries for the host round-end evaluator.
//!
//! The production model call lives in [`crate::model_evaluator`]. This module
//! stays tool-free, schema-constrained, and bounded so evaluator context cannot
//! grow without a cap.

use std::future::Future;

use codex_core::content_items_to_text;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use serde_json::Value;
use serde_json::json;

use crate::host_evaluate::GoalEvaluatorError;
use crate::host_evaluate::GoalEvaluatorVerdict;
use crate::host_evaluate::parse_goal_evaluator_verdict;

pub const EVALUATOR_SAMPLE_ATTEMPTS: usize = 2;

const TRANSCRIPT_MAX_BYTES: usize = 32 * 1024;
const ITEM_MAX_BYTES: usize = 4 * 1024;
const PLAN_MAX_BYTES: usize = 16 * 1024;

const EVALUATOR_SYSTEM_PROMPT: &str = include_str!("../templates/goals/evaluator_system.md");

pub fn evaluator_system_prompt() -> &'static str {
    EVALUATOR_SYSTEM_PROMPT
}

pub fn goal_evaluator_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["decision", "evidence", "next_step", "blocker_key"],
        "properties": {
            "decision": {
                "type": "string",
                "enum": ["continue", "candidate_complete", "blocked"]
            },
            "evidence": {
                "type": "string",
                "minLength": 1,
                "description": "Concrete transcript evidence supporting the decision"
            },
            "next_step": {
                "type": "string",
                "minLength": 1,
                "description": "One actionable next step for the agent or user"
            },
            "blocker_key": {
                "type": "string",
                "description": "Stable lowercase snake_case blocker identity for blocked; empty otherwise"
            }
        }
    })
}

pub fn build_goal_evaluator_user_payload(
    objective: &str,
    transcript: &str,
    plan: Option<&str>,
) -> String {
    json!({
        "objective": objective,
        "transcript": transcript,
        "plan": plan.unwrap_or("(no plan available)"),
    })
    .to_string()
}

/// Bounded recent transcript plus the latest `update_plan` checklist, if any.
pub fn goal_evaluator_evidence(items: &[RolloutItem]) -> GoalEvaluatorEvidence {
    let mut rows = Vec::new();
    let mut latest_plan = None;
    for item in items {
        match item {
            RolloutItem::ResponseItem(envelope) => {
                if let Some(row) = transcript_row(&envelope.item) {
                    rows.push(row);
                }
            }
            RolloutItem::EventMsg(EventMsg::PlanUpdate(plan)) => {
                latest_plan = Some(plan.clone());
            }
            RolloutItem::SessionMeta(_)
            | RolloutItem::InterAgentCommunication(_)
            | RolloutItem::InterAgentCommunicationMetadata { .. }
            | RolloutItem::Compacted(_)
            | RolloutItem::TurnContext(_)
            | RolloutItem::TokenUsageRecord(_)
            | RolloutItem::WorldState(_)
            | RolloutItem::SecurityRiskScore(_)
            | RolloutItem::EventMsg(_)
            | RolloutItem::RealtimeItem(_) => {}
        }
    }

    GoalEvaluatorEvidence {
        transcript: bound_transcript_rows(&rows),
        plan: latest_plan
            .map(|plan| truncate_bytes(&render_plan(&plan), PLAN_MAX_BYTES).to_string()),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalEvaluatorEvidence {
    pub transcript: String,
    pub plan: Option<String>,
}

pub async fn verdict_from_sample_attempts<F, Fut>(
    mut sample: F,
) -> Result<GoalEvaluatorVerdict, GoalEvaluatorError>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<String, GoalEvaluatorError>>,
{
    let mut last_error = GoalEvaluatorError::Failed("goal evaluator produced no response".into());
    for attempt in 0..EVALUATOR_SAMPLE_ATTEMPTS {
        match sample(attempt).await {
            Ok(raw) => match parse_goal_evaluator_verdict(&raw) {
                Ok(verdict) => return Ok(verdict),
                Err(error) => last_error = error.into(),
            },
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn bound_transcript_rows(rows: &[String]) -> String {
    let mut selected = Vec::new();
    let mut used = 0usize;
    for row in rows.iter().rev() {
        let row_cost = row.len().saturating_add(2);
        if !selected.is_empty() && used.saturating_add(row_cost) > TRANSCRIPT_MAX_BYTES {
            break;
        }
        used = used.saturating_add(row_cost);
        selected.push(row.as_str());
    }
    selected.reverse();
    selected.join("\n\n")
}

fn transcript_row(item: &ResponseItem) -> Option<String> {
    let (role, text) = match item {
        ResponseItem::Message { role, content, .. } => {
            if role.eq_ignore_ascii_case("system") || role.eq_ignore_ascii_case("developer") {
                return None;
            }
            let text = content_items_to_text(content)?;
            (role.as_str(), text)
        }
        ResponseItem::AgentMessage { content, .. } => {
            let text = plaintext_agent_message_content(content)?;
            ("agent_message", text)
        }
        ResponseItem::FunctionCall {
            name, arguments, ..
        } => ("tool", format!("{name} {arguments}")),
        ResponseItem::FunctionCallOutput { name, output, .. } => {
            let text = output.text_content().unwrap_or("");
            let label = name.as_deref().unwrap_or("tool");
            ("tool", format!("{label} {text}"))
        }
        ResponseItem::CustomToolCall { name, input, .. } => ("tool", format!("{name} {input}")),
        ResponseItem::CustomToolCallOutput { name, output, .. } => {
            let text = output.text_content().unwrap_or("");
            let label = name.as_deref().unwrap_or("tool");
            ("tool", format!("{label} {text}"))
        }
        ResponseItem::AdditionalTools { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::CompactionTrigger {}
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => return None,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(format!(
        "[{role}] {}",
        truncate_bytes(trimmed, ITEM_MAX_BYTES)
    ))
}

fn render_plan(plan: &UpdatePlanArgs) -> String {
    let mut lines = Vec::new();
    if let Some(explanation) = plan.explanation.as_deref() {
        let explanation = explanation.trim();
        if !explanation.is_empty() {
            lines.push(explanation.to_string());
        }
    }
    for item in &plan.plan {
        lines.push(format!(
            "[{}] {}",
            step_status_label(&item.status),
            item.step
        ));
    }
    lines.join("\n")
}

fn step_status_label(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::InProgress => "in_progress",
        StepStatus::Completed => "completed",
    }
}

fn truncate_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
