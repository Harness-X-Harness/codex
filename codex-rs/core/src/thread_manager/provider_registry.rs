use std::sync::Arc;

use codex_login::AuthManager;
use codex_model_provider::ModelProviderRegistry;
use codex_model_provider::ProviderRegistration;
use codex_model_provider::create_model_provider;
use codex_model_provider::provider_models_home;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;

use crate::config::Config;

pub(super) fn build_provider_registry(
    config: &Config,
    auth_manager: Arc<AuthManager>,
) -> CodexResult<ModelProviderRegistry> {
    let mut registrations = Vec::with_capacity(config.model_provider_registration_ids.len());
    for provider_id in &config.model_provider_registration_ids {
        let provider_info = config.model_providers.get(provider_id).ok_or_else(|| {
            CodexErr::InvalidRequest(format!(
                "registered model provider `{provider_id}` is not configured"
            ))
        })?;
        let provider =
            create_model_provider(provider_info.clone(), Some(Arc::clone(&auth_manager)));
        let config_model_catalog = if provider_id == &config.model_provider_id {
            config.model_catalog.clone()
        } else {
            None
        };
        registrations.push(ProviderRegistration::new(
            provider_id.clone(),
            if provider_id == OPENAI_PROVIDER_ID {
                "ChatGPT"
            } else {
                provider_info.name.as_str()
            },
            provider,
            provider_models_home(config.codex_home.as_path(), provider_id),
            config_model_catalog,
        ));
    }

    ModelProviderRegistry::new(registrations, &config.model_provider_id)
}
