use std::path::PathBuf;
use std::sync::Arc;

use codex_api::ApiError;
use codex_api::Provider;
use codex_api::SharedAuthProvider;
use codex_api::TransportError;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::GatewayAuthManager;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;

use crate::ResolvedResponsesProvider;
use crate::WorkspaceRoutingContext;
use crate::auth::ProviderAuthScope;
use crate::auth::ResolvedProviderAuth;
use crate::grok_catalog::static_model_catalog;
use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderCapabilities;
use crate::provider::ProviderUnauthorizedRecovery;
use crate::provider::RemoteCompactionSupport;
use crate::provider::SharedModelProvider;
use crate::provider::ToolWireFormat;

/// Grok runtime identity is selected only by the serialized wire selector.
pub(crate) fn is_grok_provider_info(provider_info: &ModelProviderInfo) -> bool {
    provider_info.wire_api == WireApi::GrokResponses
}

/// Grok runtime identity layered on top of stock configured-provider behavior.
///
/// Stock owns auth, endpoint construction, account state, and generic provider
/// lifecycle. Grok owns the release-bundled catalog and Grok capability/policy.
/// `api_provider()` does not mutate display name; dialect is mapped from
/// `WireApi` in `to_api_provider()`.
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

    fn api_provider(&self) -> ModelProviderFuture<'_, codex_protocol::error::Result<Provider>> {
        self.inner.api_provider()
    }

    fn responses_api_provider<'a>(
        &'a self,
        routing_context: &'a WorkspaceRoutingContext,
    ) -> ModelProviderFuture<'a, codex_protocol::error::Result<ResolvedResponsesProvider>> {
        self.inner.responses_api_provider(routing_context)
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

    fn tool_wire_format(&self) -> ToolWireFormat {
        ToolWireFormat::FlatFunctions
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
                ..
            } if status == "completed"
        )
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.inner.auth_manager()
    }

    fn gateway_auth_manager(&self) -> std::io::Result<Option<Arc<GatewayAuthManager>>> {
        self.inner.gateway_auth_manager()
    }

    fn supports_attestation(&self) -> bool {
        self.inner.supports_attestation()
    }

    fn is_recoverable_auth_error(&self, error: &TransportError) -> bool {
        self.inner.is_recoverable_auth_error(error)
    }

    fn recover_from_unauthorized(
        &self,
    ) -> ModelProviderFuture<'_, codex_protocol::error::Result<ProviderUnauthorizedRecovery>> {
        self.inner.recover_from_unauthorized()
    }

    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        self.inner.auth()
    }

    fn api_auth(
        &self,
    ) -> ModelProviderFuture<'_, codex_protocol::error::Result<SharedAuthProvider>> {
        self.inner.api_auth()
    }

    fn api_auth_for_scope(
        &self,
        scope: ProviderAuthScope,
    ) -> ModelProviderFuture<'_, codex_protocol::error::Result<ResolvedProviderAuth>> {
        self.inner.api_auth_for_scope(scope)
    }

    fn account_state(&self) -> ProviderAccountResult {
        self.inner.account_state()
    }

    fn map_api_error(&self, error: ApiError) -> codex_protocol::error::CodexErr {
        self.inner.map_api_error(error)
    }

    fn runtime_base_url(
        &self,
    ) -> ModelProviderFuture<'_, codex_protocol::error::Result<Option<String>>> {
        self.inner.runtime_base_url()
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
