use std::path::PathBuf;
use std::sync::Arc;

use codex_api::ResponsesDialect;
use codex_api::ImagesDialect;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
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
            image_generation: true,
            web_search: true,
            x_search: true,
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
            let projected_agent_message = match item {
                ResponseItem::AgentMessage {
                    id,
                    content,
                    internal_chat_message_metadata_passthrough,
                    ..
                } => plaintext_agent_message_content(content).map(|text| ResponseItem::Message {
                    id: id.clone(),
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText { text }],
                    phase: None,
                    internal_chat_message_metadata_passthrough:
                        internal_chat_message_metadata_passthrough.clone(),
                }),
                _ => None,
            };
            if let Some(projected_agent_message) = projected_agent_message {
                *item = projected_agent_message;
                continue;
            }
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

    fn is_provider_hosted_tool_call(&self, item: &ResponseItem) -> bool {
        matches!(
            item,
            ResponseItem::CustomToolCall {
                id: Some(id),
                status: Some(status),
                call_id,
                name,
                namespace: None,
                ..
            } if !id.is_empty()
                && !call_id.is_empty()
                && status == "completed"
                && matches!(
                    name.as_str(),
                    "x_keyword_search"
                        | "x_semantic_search"
                        | "x_user_search"
                        | "x_thread_fetch"
                )
        )
    }

    fn projects_tools_as_flat_functions(&self) -> bool {
        true
    }

    fn image_generation_model(&self) -> &'static str {
        "grok-imagine-image-2.0"
    }

    fn images_dialect(&self) -> ImagesDialect {
        ImagesDialect::Grok
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
