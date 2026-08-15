use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use codex_api::map_api_error;
use codex_http_client::HttpClientFactory;
use codex_login::AuthManager;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_models_manager::model_info::BASE_INSTRUCTIONS;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use http::HeaderMap;
use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;
use tokio::time::timeout;

use crate::models_endpoint::MODELS_REFRESH_TIMEOUT;
use crate::models_endpoint::ModelsEndpointRequest;
use crate::models_endpoint::PreparedModelsRequest;

/// Grok Gateway `/models` strategy selected by the explicit Grok Provider Adapter.
#[derive(Debug)]
pub(crate) struct GrokModelsEndpoint {
    request: ModelsEndpointRequest,
}

impl GrokModelsEndpoint {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            request: ModelsEndpointRequest::new(provider_info, auth_manager),
        }
    }

    async fn list_models(
        &self,
        client_version: &str,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<(Vec<ModelInfo>, Option<String>)> {
        let _timer =
            codex_otel::start_global_timer("codex.remote_models.fetch_update.duration_ms", &[]);
        timeout(MODELS_REFRESH_TIMEOUT, async {
            let PreparedModelsRequest {
                client,
                request_url,
            } = self
                .request
                .prepare(client_version, http_client_factory)
                .await?;
            let (body, etag) = client
                .fetch_models(request_url, HeaderMap::new())
                .await
                .map_err(map_api_error)?;
            let response =
                serde_json::from_slice::<GrokModelsResponse>(&body).map_err(|error| {
                    map_api_error(codex_api::ApiError::Stream(format!(
                        "failed to decode models response: {error}"
                    )))
                })?;
            if response.object != "list" {
                return Err(map_api_error(codex_api::ApiError::Stream(
                    "failed to decode Grok models response: expected object=list".to_string(),
                )));
            }
            let models = response
                .data
                .iter()
                .enumerate()
                .filter_map(|(index, value)| match decode_model(value) {
                    Ok(model) => Some(model),
                    Err(error) => {
                        tracing::warn!(
                            model_index = index,
                            error,
                            "skipping invalid Grok model catalog entry"
                        );
                        None
                    }
                })
                .collect();
            Ok((models, etag))
        })
        .await
        .map_err(|_| CodexErr::Timeout)?
    }
}

impl ModelsEndpointClient for GrokModelsEndpoint {
    fn has_command_auth(&self) -> bool {
        self.request.has_command_auth()
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(self.request.uses_codex_backend())
    }

    fn remote_catalog_is_authoritative(&self) -> bool {
        true
    }

    fn list_models<'a>(
        &'a self,
        client_version: &'a str,
        http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(GrokModelsEndpoint::list_models(
            self,
            client_version,
            http_client_factory,
        ))
    }
}

#[derive(Debug, Deserialize)]
struct GrokModelsResponse {
    object: String,
    data: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GrokReasoningEffortOption {
    Bare(String),
    Detailed {
        value: String,
        id: Option<String>,
        label: Option<String>,
        description: Option<String>,
        #[serde(default)]
        default: bool,
    },
}

struct DecodedReasoningEffortOption {
    preset: ReasoningEffortPreset,
    is_default: bool,
}

// Field locations are limited to the live Grok Gateway catalog and the accepted Grok Build
// `parse_remote_model_value` oracle at 8a14c91. Unknown additive fields are ignored; missing
// optional facts remain absent or unavailable.
fn decode_model(value: &Value) -> Result<ModelInfo, &'static str> {
    let object = value
        .as_object()
        .ok_or("model catalog entry must be an object")?;
    let meta = object.get("_meta").and_then(Value::as_object);
    let slug = model_slug(object, meta).ok_or("model identity must be a non-empty string")?;
    let context_window = positive_i64(
        integer_field_from(object, &["contextWindow", "context_window"]).or_else(|| {
            meta.and_then(|meta| integer_field_from(meta, &["contextWindow", "totalContextTokens"]))
        }),
    );
    let api_backend = string_field_from(object, &["apiBackend", "api_backend"]);
    let explicitly_incompatible_backend = api_backend
        .as_deref()
        .is_some_and(|backend| backend != "responses");
    let supported_in_api = bool_field_from(object, &["supportedInApi", "supported_in_api"])
        .or_else(|| meta.and_then(|meta| bool_field_from(meta, &["supportedInApi"])))
        .unwrap_or(true)
        && !explicitly_incompatible_backend;
    let hidden = bool_field_from(object, &["hidden"])
        .or_else(|| meta.and_then(|meta| bool_field_from(meta, &["hidden"])))
        .unwrap_or(false);
    let supports_reasoning_effort = bool_field_from(
        object,
        &["supportsReasoningEffort", "supports_reasoning_effort"],
    )
    .or_else(|| meta.and_then(|meta| bool_field_from(meta, &["supportsReasoningEffort"])))
    .unwrap_or(false);
    let (supported_reasoning_levels, default_reasoning_level) = if supports_reasoning_effort {
        reasoning_efforts(object, meta)
    } else {
        (Vec::new(), None)
    };
    let auto_compact_token_limit = context_window
        .zip(
            positive_i64(integer_field_from(
                object,
                &[
                    "autoCompactThresholdPercent",
                    "auto_compact_threshold_percent",
                ],
            ))
            .filter(|percent| *percent <= 100),
        )
        .map(|(context_window, percent)| context_window.saturating_mul(percent) / 100);

    Ok(ModelInfo {
        slug: slug.clone(),
        display_name: string_field_from(object, &["name"]).unwrap_or_default(),
        description: string_field_from(object, &["description"]),
        default_reasoning_level,
        supported_reasoning_levels,
        shell_type: ConfigShellToolType::Default,
        visibility: if hidden || !supported_in_api {
            ModelVisibility::Hide
        } else {
            ModelVisibility::List
        },
        supported_in_api,
        priority: 99,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        availability_nux: None,
        upgrade: None,
        model_messages: Some(ModelMessages {
            instructions_template: Some(BASE_INSTRUCTIONS.to_string()),
            instructions_variables: None,
            approvals: None,
            collaboration_modes: None,
            auto_review: None,
            permissions: None,
            token_budget: None,
        }),
        include_skills_usage_instructions: false,
        include_plugin_usage_instructions: false,
        include_apps_usage_instructions: false,
        supports_reasoning_summary_parameter: true,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: WebSearchToolType::Text,
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_parallel_tool_calls: false,
        supports_image_detail_original: false,
        context_window,
        max_context_window: context_window,
        auto_compact_token_limit,
        comp_hash: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: Vec::new(),
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        api_backend,
        supports_backend_search: bool_field_from(
            object,
            &["supportsBackendSearch", "supports_backend_search"],
        )
        .or_else(|| meta.and_then(|meta| bool_field_from(meta, &["supportsBackendSearch"])))
        .unwrap_or(false),
        use_responses_lite: false,
        auto_review_model_override: None,
        model_specialty: None,
        tool_mode: None,
        multi_agent_version: None,
    })
}

fn model_slug(object: &Map<String, Value>, meta: Option<&Map<String, Value>>) -> Option<String> {
    string_field_from(object, &["model", "modelId"])
        .or_else(|| string_field_from(object, &["id"]))
        .or_else(|| meta.and_then(|meta| string_field_from(meta, &["model", "modelId"])))
}

fn string_field_from(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn bool_field_from(object: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_bool))
}

fn integer_field_from(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_u64))
}

fn positive_i64(value: Option<u64>) -> Option<i64> {
    value
        .filter(|value| *value > 0)
        .and_then(|value| i64::try_from(value).ok())
}

fn reasoning_efforts(
    object: &Map<String, Value>,
    meta: Option<&Map<String, Value>>,
) -> (Vec<ReasoningEffortPreset>, Option<ReasoningEffort>) {
    let options = array_field_from(object, &["reasoningEfforts", "reasoning_efforts"])
        .or_else(|| meta.and_then(|meta| array_field_from(meta, &["reasoningEfforts"])))
        .map(parse_reasoning_effort_options)
        .unwrap_or_default();
    let supported = options
        .iter()
        .map(|option| option.preset.effort.clone())
        .collect::<HashSet<_>>();
    let catalog_default = string_field_from(object, &["reasoningEffort", "reasoning_effort"])
        .or_else(|| meta.and_then(|meta| string_field_from(meta, &["reasoningEffort"])))
        .and_then(|value| ReasoningEffort::from_str(&value).ok());
    let option_default = options
        .iter()
        .filter(|option| option.is_default)
        .map(|option| option.preset.effort.clone())
        .collect::<Vec<_>>();
    let default = catalog_default
        .filter(|effort| supported.contains(effort))
        .or_else(|| (option_default.len() == 1).then(|| option_default[0].clone()));
    (
        options.into_iter().map(|option| option.preset).collect(),
        default,
    )
}

fn array_field_from<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a [Value]> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_array))
        .map(Vec::as_slice)
}

fn parse_reasoning_effort_options(values: &[Value]) -> Vec<DecodedReasoningEffortOption> {
    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let raw = match serde_json::from_value::<GrokReasoningEffortOption>(value.clone()) {
                Ok(raw) => raw,
                Err(error) => {
                    tracing::warn!(
                        option_index = index,
                        %error,
                        "skipping invalid Grok reasoning effort option"
                    );
                    return None;
                }
            };
            let (value, description, is_default) = match raw {
                GrokReasoningEffortOption::Bare(value) => {
                    let description = value.clone();
                    (value, description, false)
                }
                GrokReasoningEffortOption::Detailed {
                    value,
                    id,
                    label,
                    description,
                    default,
                } => {
                    let description = description
                        .or(label)
                        .or(id)
                        .unwrap_or_else(|| value.clone());
                    (value, description, default)
                }
            };
            let effort = match ReasoningEffort::from_str(value.trim()) {
                Ok(effort) => effort,
                Err(error) => {
                    tracing::warn!(
                        option_index = index,
                        error,
                        "skipping invalid Grok reasoning effort option"
                    );
                    return None;
                }
            };
            Some(DecodedReasoningEffortOption {
                preset: ReasoningEffortPreset {
                    effort,
                    description,
                },
                is_default,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "grok_models_endpoint_tests.rs"]
mod tests;
