use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::MultiAgentVersion;

const GROK_4_6_MODEL_ID: &str = "grok-4.6";
const GROK_CONTEXT_WINDOW: i64 = 500_000;
const GROK_AUTO_COMPACT_TOKEN_LIMIT: i64 = 400_000;

/// Returns the complete Grok model catalog bundled with this release.
///
/// Remote catalog observations are release inputs, not a runtime dependency.
pub(crate) fn static_model_catalog() -> ModelsResponse {
    ModelsResponse {
        models: vec![ModelInfo {
            slug: GROK_4_6_MODEL_ID.to_string(),
            display_name: "Grok 4.6".to_string(),
            description: Some("Grok 4.6 model".to_string()),
            default_reasoning_level: Some(ReasoningEffort::High),
            supported_reasoning_levels: vec![
                reasoning_effort(ReasoningEffort::Ultra, "Ultra reasoning"),
                reasoning_effort(ReasoningEffort::XHigh, "Maximum reasoning"),
                reasoning_effort(ReasoningEffort::High, "High reasoning"),
                reasoning_effort(ReasoningEffort::Medium, "Medium reasoning"),
                reasoning_effort(ReasoningEffort::Low, "Low reasoning"),
            ],
            shell_type: ConfigShellToolType::Default,
            visibility: ModelVisibility::List,
            supported_in_api: true,
            priority: 0,
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
            context_window: Some(GROK_CONTEXT_WINDOW),
            max_context_window: Some(GROK_CONTEXT_WINDOW),
            auto_compact_token_limit: Some(GROK_AUTO_COMPACT_TOKEN_LIMIT),
            comp_hash: None,
            effective_context_window_percent: 95,
            experimental_supported_tools: Vec::new(),
            input_modalities: vec![InputModality::Text, InputModality::Image],
            used_fallback_model_metadata: false,
            supports_search_tool: false,
            use_responses_lite: false,
            node_repl_auto_review_required: false,
            node_repl_disabled: false,
            auto_review_model_override: None,
            model_specialty: None,
            tool_mode: None,
            multi_agent_version: Some(MultiAgentVersion::V2),
        }],
    }
}

fn reasoning_effort(effort: ReasoningEffort, description: &str) -> ReasoningEffortPreset {
    ReasoningEffortPreset {
        effort,
        description: description.to_string(),
    }
}
