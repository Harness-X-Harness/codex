mod amazon_bedrock;
mod auth;
mod bearer_auth_provider;
mod combined_auth;
mod grok_catalog;
mod grok_provider;
mod models_endpoint;
mod models_identity;
mod provider;
mod shared_state;
pub mod test_support;
mod workspace_routing;
pub use workspace_routing::ACCOUNT_ROUTING_HEADER;
pub use workspace_routing::ResolvedResponsesProvider;
pub use workspace_routing::ResponsesConnectionKey;
pub use workspace_routing::WorkspaceRoutingContext;

pub use amazon_bedrock::is_supported_amazon_bedrock_region;
pub use auth::AgentIdentitySessionFallback;
pub use auth::ProviderAuthScope;
pub use auth::ResolvedProviderAuth;
pub use auth::auth_provider_from_auth;
pub use auth::auth_provider_from_auth_manager;
pub use auth::unauthenticated_auth_provider;
pub use bearer_auth_provider::BearerAuthProvider;
pub use bearer_auth_provider::BearerAuthProvider as CoreAuthProvider;
pub use codex_model_provider_info::AMAZON_BEDROCK_PROVIDER_ID;
pub use codex_model_provider_info::AMAZON_BEDROCK_RUNTIME_PROVIDER_ID;
pub use codex_model_provider_info::CHATGPT_CODEX_BASE_URL;
pub use codex_protocol::account::ProviderAccount;
pub use grok_catalog::GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES;
pub use provider::ModelProvider;
pub use provider::ModelProviderFuture;
pub use provider::ProviderAccountError;
pub use provider::ProviderAccountResult;
pub use provider::ProviderAccountState;
pub use provider::ProviderAuthRecoveryMessages;
pub use provider::ProviderCapabilities;
pub use provider::ProviderUnauthorizedRecovery;
pub use provider::RemoteCompactionSupport;
pub use provider::SharedModelProvider;
pub use provider::ToolWireFormat;

const DEFAULT_IMAGE_GENERATION_MAX_EDIT_IMAGES: usize = 5;

/// Provider-owned image-generation policy consumed by the stock image extension.
///
/// The policy controls only whether the extension may be exposed and the provider's
/// edit-image cardinality. Request/response wire projection remains an API-boundary concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageGenerationPolicy {
    pub max_edit_images: usize,
}

/// Resolves image-generation policy without making the image extension guess identity.
///
/// Stock availability stays OpenAI-auth-or-actor. Grok opts in only through
/// `WireApi::GrokResponses` plus its image capability. Images consume the same
/// `ApiDialect` as Responses.
pub fn image_generation_policy(provider: &SharedModelProvider) -> Option<ImageGenerationPolicy> {
    let info = provider.info();
    let is_grok = grok_provider::is_grok_provider_info(info);
    let stock_available =
        info.is_openai() || info.requires_openai_auth || info.uses_openai_actor_authorization();
    if !provider.capabilities().image_generation || (!stock_available && !is_grok) {
        return None;
    }

    Some(ImageGenerationPolicy {
        max_edit_images: if is_grok {
            GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES
        } else {
            DEFAULT_IMAGE_GENERATION_MAX_EDIT_IMAGES
        },
    })
}

/// Creates the runtime model provider for configured provider metadata.
///
/// Grok is selected only by `WireApi::GrokResponses`. Stock keeps ownership of
/// auth, transport, account state, and generic provider behavior. The Grok
/// wrapper owns the release-bundled catalog and Grok capability/policy.
pub fn create_model_provider(
    provider_info: codex_model_provider_info::ModelProviderInfo,
    auth_manager: Option<std::sync::Arc<codex_login::AuthManager>>,
) -> provider::SharedModelProvider {
    if grok_provider::is_grok_provider_info(&provider_info) {
        std::sync::Arc::new(grok_provider::GrokModelProvider::new(
            provider_info,
            auth_manager,
        ))
    } else {
        provider::create_model_provider(provider_info, auth_manager)
    }
}

#[cfg(test)]
#[path = "workspace_routing_tests.rs"]
mod workspace_routing_tests;
