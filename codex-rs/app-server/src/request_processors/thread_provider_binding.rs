use super::request_errors::provider_binding_error;
use crate::error_code::invalid_params;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_models_manager::manager::RefreshStrategy;
use serde_json::Value;
use std::collections::HashMap;

/// Explicit provider-selection inputs captured before persisted resume metadata is merged.
#[derive(Debug, Default)]
pub(super) struct ProviderSelectionOverrides {
    model: Option<String>,
    provider_id: Option<String>,
}

impl ProviderSelectionOverrides {
    pub(super) fn capture(
        request_overrides: Option<&HashMap<String, Value>>,
        typesafe_overrides: &ConfigOverrides,
    ) -> Result<Self, JSONRPCErrorError> {
        Ok(Self {
            model: selected_string_override(
                "model",
                typesafe_overrides.model.as_deref(),
                request_overrides,
            )?,
            provider_id: selected_string_override(
                "model_provider",
                typesafe_overrides.model_provider.as_deref(),
                request_overrides,
            )?,
        })
    }
}

pub(super) struct ExistingThreadProviderBinding<'a> {
    pub provider_id: &'a str,
    pub model: Option<&'a str>,
}

/// Compile explicit overrides and persisted metadata into one immutable provider binding.
pub(super) async fn apply_existing_thread_provider_binding(
    thread_manager: &ThreadManager,
    config: &Config,
    binding: ExistingThreadProviderBinding<'_>,
    selection_overrides: ProviderSelectionOverrides,
    request_overrides: &mut Option<HashMap<String, Value>>,
    typesafe_overrides: &mut ConfigOverrides,
) -> Result<(), JSONRPCErrorError> {
    let Some(selection) = thread_manager
        .resolve_existing_thread_provider(
            binding.provider_id,
            binding.model,
            selection_overrides.model.as_deref(),
            selection_overrides.provider_id.as_deref(),
            RefreshStrategy::OnlineIfUncached,
            config.http_client_factory(),
        )
        .await
        .map_err(provider_binding_error)?
    else {
        return Ok(());
    };

    if let Some(request_overrides) = request_overrides.as_mut() {
        request_overrides.remove("model");
        request_overrides.remove("model_provider");
    }
    typesafe_overrides.model = Some(selection.model);
    typesafe_overrides.model_provider = Some(selection.provider_id);
    Ok(())
}

/// Reject a running-thread resume that requests a provider other than the
/// thread's immutable provider binding.
pub(super) async fn validate_existing_thread_provider_binding(
    thread_manager: &ThreadManager,
    config: &Config,
    binding: ExistingThreadProviderBinding<'_>,
    selection_overrides: ProviderSelectionOverrides,
) -> Result<(), JSONRPCErrorError> {
    if selection_overrides.model.is_none() && selection_overrides.provider_id.is_none() {
        return Ok(());
    }

    thread_manager
        .resolve_existing_thread_provider(
            binding.provider_id,
            binding.model,
            selection_overrides.model.as_deref(),
            selection_overrides.provider_id.as_deref(),
            RefreshStrategy::OnlineIfUncached,
            config.http_client_factory(),
        )
        .await
        .map_err(provider_binding_error)?;
    Ok(())
}

pub(super) async fn validate_existing_thread_model_update(
    thread_manager: &ThreadManager,
    config: &Config,
    bound_provider_id: &str,
    current_model: &str,
    requested_model: &str,
) -> Result<(), JSONRPCErrorError> {
    thread_manager
        .resolve_existing_thread_provider(
            bound_provider_id,
            Some(current_model),
            Some(requested_model),
            /*requested_provider_id*/ None,
            RefreshStrategy::OnlineIfUncached,
            config.http_client_factory(),
        )
        .await
        .map_err(provider_binding_error)?;
    Ok(())
}

fn selected_string_override(
    key: &str,
    typesafe_value: Option<&str>,
    request_overrides: Option<&HashMap<String, Value>>,
) -> Result<Option<String>, JSONRPCErrorError> {
    let config_value = request_overrides
        .and_then(|overrides| overrides.get(key))
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                invalid_params(format!("`config.{key}` must be a string when provided"))
            })
        })
        .transpose()?;
    if let (Some(typesafe_value), Some(config_value)) = (typesafe_value, config_value.as_deref())
        && typesafe_value != config_value
    {
        return Err(invalid_params(format!(
            "conflicting `{key}` and `config.{key}` overrides"
        )));
    }
    Ok(typesafe_value.map(str::to_string).or(config_value))
}
