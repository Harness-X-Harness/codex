use std::path::PathBuf;
use std::sync::Arc;

use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelsResponse;

use crate::grok_models_endpoint::GrokModelsEndpoint;
use crate::provider::ConfiguredModelProvider;
use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderAccountState;
use crate::provider::ProviderCapabilities;
use crate::provider::RemoteCompactionSupport;

/// Official Grok Gateway Provider Adapter.
///
/// This adapter owns Grok catalog authority and capability policy. The Grok
/// wire dialect remains an explicit, separate `ModelProviderInfo::wire_api`.
#[derive(Clone, Debug)]
pub(crate) struct GrokModelProvider {
    common: ConfiguredModelProvider,
}

impl GrokModelProvider {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            common: ConfiguredModelProvider::new(provider_info, auth_manager),
        }
    }

    fn models_endpoint(&self) -> Arc<GrokModelsEndpoint> {
        Arc::new(GrokModelsEndpoint::new(
            self.info().clone(),
            self.auth_manager(),
        ))
    }
}

impl ModelProvider for GrokModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        self.common.info()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            remote_compaction: RemoteCompactionSupport::Unsupported,
            ..ProviderCapabilities::default()
        }
    }

    fn approval_review_preferred_model(&self) -> &'static str {
        self.common.approval_review_preferred_model()
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.common.auth_manager()
    }

    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        self.common.auth()
    }

    fn account_state(&self) -> ProviderAccountResult {
        Ok(ProviderAccountState {
            account: None,
            requires_openai_auth: false,
        })
    }

    fn models_manager(
        &self,
        codex_home: PathBuf,
        _config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::new(OpenAiModelsManager::new(
            codex_home,
            self.models_endpoint(),
            self.auth_manager(),
        ))
    }

    fn models_manager_without_cache(
        &self,
        _config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::new(OpenAiModelsManager::new_without_cache(
            self.models_endpoint(),
            self.auth_manager(),
        ))
    }

    fn models_manager_with_cache(
        &self,
        _config_model_catalog: Option<ModelsResponse>,
        cache: Arc<dyn ModelsCache>,
    ) -> SharedModelsManager {
        Arc::new(OpenAiModelsManager::new_with_cache(
            cache,
            self.models_endpoint(),
            self.auth_manager(),
        ))
    }
}
