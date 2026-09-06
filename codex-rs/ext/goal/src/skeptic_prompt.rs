//! Prompt construction for host skeptic sessions.

use serde_json::Value;
use serde_json::json;

use crate::policy::HOST_SKEPTIC_MAX_COUNT;
use crate::policy::HOST_SKEPTIC_MIN_COUNT;

const SKEPTIC_SYSTEM_PROMPT: &str = include_str!("../templates/goals/skeptic_system.md");

pub fn skeptic_system_prompt() -> &'static str {
    SKEPTIC_SYSTEM_PROMPT
}

pub fn clamp_host_skeptic_count(count: u8) -> u8 {
    count.clamp(HOST_SKEPTIC_MIN_COUNT, HOST_SKEPTIC_MAX_COUNT)
}

pub fn goal_skeptic_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["refuted", "evidence", "next_step"],
        "properties": {
            "refuted": {
                "type": "boolean",
                "description": "True when completion is not proven"
            },
            "evidence": {
                "type": "string",
                "minLength": 1,
                "description": "Concrete evidence for the vote"
            },
            "next_step": {
                "type": "string",
                "minLength": 1,
                "description": "Actionable fix when refuted; none when confirmed"
            }
        }
    })
}

pub fn build_goal_skeptic_user_payload(
    skeptic_index: u8,
    panel_size: u8,
    objective: &str,
    candidate_next_step: &str,
    transcript: &str,
    plan: Option<&str>,
) -> String {
    json!({
        "skeptic_index": skeptic_index,
        "panel_size": panel_size,
        "objective": objective,
        "candidate_next_step": candidate_next_step,
        "transcript": transcript,
        "plan": plan.unwrap_or("(no plan available)"),
    })
    .to_string()
}
