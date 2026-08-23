use std::path::PathBuf;
use std::sync::Arc;

use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelsResponse;

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
}

impl ModelProvider for GrokModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        self.inner.info()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
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
        codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.inner.models_manager(codex_home, config_model_catalog)
    }

    fn models_manager_without_cache(
        &self,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.inner
            .models_manager_without_cache(config_model_catalog)
    }

    fn models_manager_with_cache(
        &self,
        config_model_catalog: Option<ModelsResponse>,
        cache: Arc<dyn ModelsCache>,
    ) -> SharedModelsManager {
        self.inner
            .models_manager_with_cache(config_model_catalog, cache)
    }
}
