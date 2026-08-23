use std::collections::HashSet;

use codex_api::ApiError;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::MultiAgentVersion;
use serde_json::Map;
use serde_json::Value;

use crate::models_endpoint::ModelsResponseDecoder;

#[derive(Debug)]
pub(crate) struct GrokModelsResponseDecoder;

impl ModelsResponseDecoder for GrokModelsResponseDecoder {
    fn decode(&self, body: &[u8]) -> Result<Vec<ModelInfo>, ApiError> {
        decode_grok_models(body).map_err(|err| {
            ApiError::Stream(format!("failed to decode Grok models response: {err}"))
        })
    }
}

fn decode_grok_models(body: &[u8]) -> Result<Vec<ModelInfo>, String> {
    let response: Value = serde_json::from_slice(body).map_err(|err| err.to_string())?;
    let fields = response
        .as_object()
        .ok_or_else(|| "catalog response must be an object".to_string())?;
    let object = required_string(fields, "object")?;
    if object != "list" {
        return Err(format!(
            "catalog response object must be list, got {object}"
        ));
    }
    let entries = fields
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "top-level data must be an array".to_string())?;

    let mut slugs = HashSet::with_capacity(entries.len());
    entries
        .iter()
        .enumerate()
        .map(|(priority, entry)| {
            let model = decode_grok_model(entry, priority)?;
            if !slugs.insert(model.slug.clone()) {
                return Err(format!("duplicate model id: {}", model.slug));
            }
            Ok(model)
        })
        .collect()
}

fn decode_grok_model(entry: &Value, priority: usize) -> Result<ModelInfo, String> {
    let fields = entry
        .as_object()
        .ok_or_else(|| "catalog entry must be an object".to_string())?;
    let id = required_string(fields, "id")?;
    let model = required_string(fields, "model")?;
    if id != model {
        return Err(format!(
            "catalog entry id {id} does not match model {model}"
        ));
    }

    let context_window = required_positive_i64(fields, "context_window")?;
    let default_reasoning_level = required_string(fields, "reasoning_effort")?
        .parse::<ReasoningEffort>()
        .map_err(|err| format!("model {id} has invalid reasoning_effort: {err}"))?;
    let supported_reasoning_levels = decode_reasoning_efforts(fields, &id)?;
    if !supported_reasoning_levels
        .iter()
        .any(|preset| preset.effort == default_reasoning_level)
    {
        return Err(format!(
            "model {id} default reasoning effort {default_reasoning_level} is not supported"
        ));
    }

    let auto_compact_token_limit = optional_percentage(fields, "auto_compact_threshold_percent")?
        .map(|percent| {
            context_window
                .checked_mul(percent)
                .ok_or_else(|| format!("model {id} compaction threshold overflow"))
                .map(|tokens| tokens / 100)
        })
        .transpose()?;
    let multi_agent_version =
        (id == "grok-4.6").then_some(MultiAgentVersion::V2);

    Ok(ModelInfo {
        slug: id,
        display_name: required_string(fields, "name")?,
        description: optional_string(fields, "description")?,
        default_reasoning_level: Some(default_reasoning_level),
        supported_reasoning_levels,
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority: i32::try_from(priority).map_err(|_| "catalog has too many models".to_string())?,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        availability_nux: None,
        upgrade: None,
        model_messages: None,
        include_skills_usage_instructions: false,
        include_plugin_usage_instructions: false,
        include_apps_usage_instructions: false,
        supports_reasoning_summary_parameter: false,
        default_reasoning_summary: ReasoningSummary::None,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: WebSearchToolType::Text,
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_image_detail_original: false,
        context_window: Some(context_window),
        max_context_window: Some(context_window),
        auto_compact_token_limit,
        comp_hash: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: vec![InputModality::Text],
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        use_responses_lite: false,
        node_repl_auto_review_required: false,
        node_repl_disabled: false,
        auto_review_model_override: None,
        model_specialty: None,
        tool_mode: None,
        multi_agent_version,
    })
}

fn decode_reasoning_efforts(
    fields: &Map<String, Value>,
    model_id: &str,
) -> Result<Vec<ReasoningEffortPreset>, String> {
    let efforts = fields
        .get("reasoning_efforts")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("model {model_id} reasoning_efforts must be an array"))?;
    if efforts.is_empty() {
        return Err(format!(
            "model {model_id} reasoning_efforts must not be empty"
        ));
    }

    let mut seen = HashSet::with_capacity(efforts.len());
    efforts
        .iter()
        .map(|effort| {
            let effort_fields = effort
                .as_object()
                .ok_or_else(|| format!("model {model_id} reasoning effort must be an object"))?;
            let value = required_string(effort_fields, "value")?;
            let effort = value
                .parse::<ReasoningEffort>()
                .map_err(|err| format!("model {model_id} has invalid reasoning effort: {err}"))?;
            if !seen.insert(effort.clone()) {
                return Err(format!(
                    "model {model_id} has duplicate reasoning effort {effort}"
                ));
            }
            Ok(ReasoningEffortPreset {
                effort,
                description: required_string(effort_fields, "description")?,
            })
        })
        .collect()
}

fn required_string(fields: &Map<String, Value>, field: &str) -> Result<String, String> {
    let value = fields
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} must be a non-empty string"))?;
    Ok(value.to_string())
}

fn optional_string(fields: &Map<String, Value>, field: &str) -> Result<Option<String>, String> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{field} must be a string or null")),
    }
}

fn required_positive_i64(fields: &Map<String, Value>, field: &str) -> Result<i64, String> {
    fields
        .get(field)
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{field} must be a positive integer"))
}

fn optional_percentage(fields: &Map<String, Value>, field: &str) -> Result<Option<i64>, String> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|percent| (1..=100).contains(percent))
            .map(Some)
            .ok_or_else(|| format!("{field} must be an integer from 1 through 100")),
    }
}

#[cfg(test)]
#[path = "grok_catalog_tests.rs"]
mod tests;
