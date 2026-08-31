use std::path::PathBuf;
use std::sync::Arc;

use codex_api::ResponsesDialect;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;

use crate::grok_catalog::static_model_catalog;
use crate::provider::ConfiguredModelProvider;
use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderCapabilities;
use crate::provider::RemoteCompactionSupport;

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
        Arc::new(StaticModelsManager::new(
            auth_manager,
            config_model_catalog.unwrap_or_else(static_model_catalog),
        ))
    }
}

impl ModelProvider for GrokModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        self.inner.info()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            namespace_tools: true,
            image_generation: false,
            web_search: true,
            cached_web_search: false,
            external_web_access: true,
            indexed_web_search: false,
            remote_compaction: RemoteCompactionSupport::Unsupported,
        }
    }

    fn project_reasoning_effort(&self, effort: ReasoningEffort) -> ReasoningEffort {
        match effort {
            ReasoningEffort::Ultra => ReasoningEffort::XHigh,
            effort => effort,
        }
    }

    fn project_model_input(&self, input: Vec<ResponseItem>) -> Vec<ResponseItem> {
        let mut input = self.inner.project_model_input(input);
        for item in &mut input {
            if let ResponseItem::Reasoning {
                content,
                encrypted_content: Some(_),
                ..
            } = item
                && content.is_none()
            {
                *content = Some(Vec::new());
            }
        }
        input
    }

    fn projects_tools_as_flat_functions(&self) -> bool {
        true
    }

    fn approval_review_preferred_model(&self) -> Option<&'static str> {
        None
    }

    fn memory_extraction_preferred_model(&self) -> Option<&'static str> {
        None
    }

    fn memory_consolidation_preferred_model(&self) -> Option<&'static str> {
        None
    }

    fn supports_attestation(&self) -> bool {
        false
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

    fn api_provider(
        &self,
    ) -> ModelProviderFuture<'_, codex_protocol::error::Result<codex_api::Provider>> {
        Box::pin(async move {
            let mut provider = self.inner.api_provider().await?;
            provider.responses_dialect = ResponsesDialect::Grok;
            Ok(provider)
        })
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

#[cfg(test)]
#[path = "grok_provider_tests.rs"]
mod tests;
