use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn effort(effort: ReasoningEffort, description: &str) -> ReasoningEffortPreset {
    ReasoningEffortPreset {
        effort,
        description: description.to_string(),
    }
}

fn expected_model(
    slug: &str,
    display_name: &str,
    description: &str,
    priority: i32,
    supported_reasoning_levels: Vec<ReasoningEffortPreset>,
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        default_reasoning_level: Some(ReasoningEffort::High),
        supported_reasoning_levels,
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority,
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
        context_window: Some(500_000),
        max_context_window: Some(500_000),
        auto_compact_token_limit: Some(400_000),
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
        multi_agent_version: None,
    }
}

#[test]
fn current_gateway_catalog_projects_to_exact_stock_model_info() {
    let body = serde_json::to_vec(&json!({
        "object": "list",
        "data": [
            {
                "id": "grok-4.6",
                "model": "grok-4.6",
                "name": "Grok 4.6",
                "description": "Grok 4.6 model",
                "context_window": 500_000,
                "reasoning_effort": "high",
                "reasoning_efforts": [
                    {"id": "xhigh", "label": "XHigh", "value": "xhigh", "description": "Maximum reasoning", "default": false},
                    {"id": "high", "label": "High", "value": "high", "description": "High reasoning", "default": true},
                    {"id": "medium", "label": "Medium", "value": "medium", "description": "Medium reasoning", "default": false},
                    {"id": "low", "label": "Low", "value": "low", "description": "Low reasoning", "default": false}
                ],
                "auto_compact_threshold_percent": 80
            },
            {
                "id": "grok-4.5",
                "model": "grok-4.5",
                "name": "Grok 4.5",
                "description": "Grok 4.5 model",
                "context_window": 500_000,
                "reasoning_effort": "high",
                "reasoning_efforts": [
                    {"id": "high", "label": "High", "value": "high", "description": "High reasoning", "default": true},
                    {"id": "medium", "label": "Medium", "value": "medium", "description": "Medium reasoning", "default": false},
                    {"id": "low", "label": "Low", "value": "low", "description": "Low reasoning", "default": false}
                ],
                "auto_compact_threshold_percent": 80
            }
        ]
    }))
    .expect("fixture should serialize");

    assert_eq!(
        decode_grok_models(&body),
        Ok(vec![
            expected_model(
                "grok-4.6",
                "Grok 4.6",
                "Grok 4.6 model",
                0,
                vec![
                    effort(ReasoningEffort::XHigh, "Maximum reasoning"),
                    effort(ReasoningEffort::High, "High reasoning"),
                    effort(ReasoningEffort::Medium, "Medium reasoning"),
                    effort(ReasoningEffort::Low, "Low reasoning"),
                ],
            ),
            expected_model(
                "grok-4.5",
                "Grok 4.5",
                "Grok 4.5 model",
                1,
                vec![
                    effort(ReasoningEffort::High, "High reasoning"),
                    effort(ReasoningEffort::Medium, "Medium reasoning"),
                    effort(ReasoningEffort::Low, "Low reasoning"),
                ],
            ),
        ])
    );
}
