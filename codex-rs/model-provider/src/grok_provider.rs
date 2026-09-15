use std::path::PathBuf;
use std::sync::Arc;

use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;

use crate::grok_catalog::static_model_catalog;
use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderCapabilities;
use crate::provider::RemoteCompactionSupport;
use crate::provider::SharedModelProvider;

pub(crate) const GROK_PROVIDER_NAME: &str = "Grok";

pub(crate) fn is_grok_provider_info(provider_info: &ModelProviderInfo) -> bool {
    provider_info.wire_api == WireApi::GrokResponses
        || provider_info.name.eq_ignore_ascii_case(GROK_PROVIDER_NAME)
}

/// Grok runtime identity layered on top of stock configured-provider behavior.
///
/// Stock owns auth, endpoint construction, account state, and generic provider
/// lifecycle. Grok owns only the release-bundled catalog in this migration seam.
#[derive(Clone, Debug)]
pub(crate) struct GrokModelProvider {
    inner: SharedModelProvider,
}

impl GrokModelProvider {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            inner: crate::provider::create_model_provider(provider_info, auth_manager),
        }
    }

    fn authoritative_models_manager(
        &self,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::new(StaticModelsManager::new(
            self.inner.auth_manager(),
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
            image_generation: true,
            web_search: true,
            external_web_access: true,
            remote_compaction: RemoteCompactionSupport::Unsupported,
        }
    }

    fn projects_tools_as_flat_functions(&self) -> bool {
        true
    }

    fn approval_review_preferred_model(&self) -> &'static str {
        crate::grok_catalog::GROK_4_6_MODEL_ID
    }

    fn memory_extraction_preferred_model(&self) -> &'static str {
        crate::grok_catalog::GROK_4_6_MODEL_ID
    }

    fn memory_consolidation_preferred_model(&self) -> &'static str {
        crate::grok_catalog::GROK_4_6_MODEL_ID
    }

    fn is_provider_hosted_tool_call(&self, item: &ResponseItem) -> bool {
        matches!(
            item,
            ResponseItem::CustomToolCall {
                status: Some(status),
                name,
                ..
            } if status == "completed"
                && matches!(
                    name.as_str(),
                    "x_keyword_search"
                        | "x_semantic_search"
                        | "x_user_search"
                        | "x_thread_fetch"
                )
        )
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

#[cfg(test)]
#[path = "grok_provider_tests.rs"]
mod tests;
