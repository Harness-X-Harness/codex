use std::path::PathBuf;
use std::sync::Arc;

use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;

use crate::grok_catalog::GrokModelsResponseDecoder;
use crate::models_endpoint::OpenAiModelsEndpoint;
use crate::provider::ConfiguredModelProvider;
use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderCapabilities;

/// Grok provider identity using the configured provider lifecycle.
#[derive(Clone, Debug)]
pub(crate) struct GrokModelProvider {
    inner: ConfiguredModelProvider,
}

impl GrokModelProvider {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            inner: ConfiguredModelProvider::new(provider_info, auth_manager),
        }
    }

    fn authoritative_models_manager(
        &self,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        let auth_manager = self.inner.auth_manager();
        match config_model_catalog {
            Some(model_catalog) => Arc::new(StaticModelsManager::new(auth_manager, model_catalog)),
            None => {
                let endpoint = Arc::new(OpenAiModelsEndpoint::new_with_decoder(
                    self.inner.info().clone(),
                    auth_manager.clone(),
                    Arc::new(GrokModelsResponseDecoder),
                ));
                Arc::new(OpenAiModelsManager::new_authoritative_without_cache(
                    endpoint,
                    auth_manager,
                ))
            }
        }
    }
}

impl ModelProvider for GrokModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        self.inner.info()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    fn project_reasoning_effort(&self, effort: ReasoningEffort) -> ReasoningEffort {
        match effort {
            ReasoningEffort::Ultra => ReasoningEffort::XHigh,
            effort => effort,
        }
    }

    fn approval_review_preferred_model(&self) -> &'static str {
        self.inner.approval_review_preferred_model()
    }

    fn supports_attestation(&self) -> bool {
        self.inner.supports_attestation()
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.inner.auth_manager()
    }

    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        self.inner.auth()
    }

    fn account_state(&self) -> ProviderAccountResult {
        self.inner.account_state()
    }

    fn models_manager(
        &self,
        _codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.authoritative_models_manager(config_model_catalog)
    }

    fn models_manager_without_cache(
        &self,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.authoritative_models_manager(config_model_catalog)
    }

    fn models_manager_with_cache(
        &self,
        config_model_catalog: Option<ModelsResponse>,
        _cache: Arc<dyn ModelsCache>,
    ) -> SharedModelsManager {
        self.authoritative_models_manager(config_model_catalog)
    }
}
