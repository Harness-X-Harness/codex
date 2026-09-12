mod amazon_bedrock;
mod auth;
mod bearer_auth_provider;
mod grok_catalog;
mod grok_provider;
mod models_endpoint;
mod provider;
mod shared_state;

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

/// Creates the runtime model provider for configured provider metadata.
///
/// Grok is a thin product-specific wrapper around the stock configured provider:
/// stock keeps ownership of auth, transport, account state, and generic provider
/// behavior, while the Grok wrapper owns only the release-bundled model catalog.
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
