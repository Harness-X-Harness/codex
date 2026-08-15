use std::sync::Arc;

use codex_api::AgentIdentityTelemetry;
use codex_api::AuthProvider;
use codex_api::Provider;
use codex_api::SharedAuthProvider;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::auth::AuthMode;
use codex_protocol::error::Result;
use sha2::Digest;
use sha2::Sha256;

use crate::ProviderAuthScope;
use crate::auth::ResolvedProviderAuth;
use crate::auth::resolve_provider_auth;
use crate::auth::resolve_provider_auth_for_scope;

/// Opaque identity of the endpoint and credential strategy selected for a Turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRequestStrategy([u8; 32]);

impl ProviderRequestStrategy {
    fn from_provider(
        api_provider: &Provider,
        auth: Option<&CodexAuth>,
        api_auth: &dyn AuthProvider,
        agent_identity_telemetry: Option<&AgentIdentityTelemetry>,
    ) -> Self {
        let mut hasher = Sha256::new();
        update_digest(&mut hasher, b"codex-provider-request-strategy-v1");
        update_digest(&mut hasher, api_provider.base_url.as_bytes());
        update_digest(
            &mut hasher,
            match api_provider.responses_dialect {
                codex_api::ResponsesDialect::OpenAi => b"openai",
                codex_api::ResponsesDialect::Grok => b"grok",
            },
        );
        update_auth_identity(&mut hasher, auth, api_auth, agent_identity_telemetry);

        let mut query_params = api_provider
            .query_params
            .iter()
            .flat_map(|params| params.iter())
            .collect::<Vec<_>>();
        query_params.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (name, value) in query_params {
            update_digest(&mut hasher, name.as_bytes());
            update_digest(&mut hasher, value.as_bytes());
        }

        let mut headers = api_provider.headers.iter().collect::<Vec<_>>();
        headers.sort_by(|(left_name, left_value), (right_name, right_value)| {
            left_name
                .as_str()
                .cmp(right_name.as_str())
                .then_with(|| left_value.as_bytes().cmp(right_value.as_bytes()))
        });
        for (name, value) in headers {
            update_digest(&mut hasher, name.as_str().as_bytes());
            update_digest(&mut hasher, value.as_bytes());
        }

        Self(hasher.finalize().into())
    }
}

fn update_auth_identity(
    hasher: &mut Sha256,
    auth: Option<&CodexAuth>,
    api_auth: &dyn AuthProvider,
    agent_identity_telemetry: Option<&AgentIdentityTelemetry>,
) {
    let auth_mode = auth.map(CodexAuth::api_auth_mode);
    let effective_auth_kind = if agent_identity_telemetry.is_some() {
        "agent_identity"
    } else {
        match auth_mode {
            Some(AuthMode::ApiKey) => "api_key",
            Some(AuthMode::Chatgpt) => "chatgpt",
            Some(AuthMode::ChatgptAuthTokens) => "chatgpt_auth_tokens",
            Some(AuthMode::Headers) => "headers",
            Some(AuthMode::AgentIdentity) => "agent_identity",
            Some(AuthMode::PersonalAccessToken) => "personal_access_token",
            Some(AuthMode::BedrockApiKey) => "bedrock_api_key",
            None => "provider_auth",
        }
    };
    update_digest(hasher, effective_auth_kind.as_bytes());

    if let Some(auth) = auth {
        update_digest(hasher, auth.get_account_id().unwrap_or_default().as_bytes());
        update_digest(
            hasher,
            auth.get_chatgpt_user_id().unwrap_or_default().as_bytes(),
        );
        update_digest(hasher, &[u8::from(auth.is_workspace_account())]);
    }
    if let Some(agent_identity) = agent_identity_telemetry {
        update_digest(hasher, agent_identity.agent_id.as_bytes());
        update_digest(hasher, agent_identity.task_id.as_bytes());
    }

    if !matches!(
        auth_mode,
        Some(AuthMode::Chatgpt | AuthMode::ChatgptAuthTokens | AuthMode::PersonalAccessToken)
    ) && agent_identity_telemetry.is_none()
    {
        let auth_headers = api_auth.to_auth_headers();
        let mut auth_headers = auth_headers.iter().collect::<Vec<_>>();
        auth_headers.sort_by(|(left_name, left_value), (right_name, right_value)| {
            left_name
                .as_str()
                .cmp(right_name.as_str())
                .then_with(|| left_value.as_bytes().cmp(right_value.as_bytes()))
        });
        for (name, value) in auth_headers {
            update_digest(hasher, name.as_str().as_bytes());
            update_digest(hasher, value.as_bytes());
        }
    }
}

fn update_digest(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

/// Provider route and credentials resolved coherently for one request attempt.
pub struct ProviderRequestSetup {
    pub auth: Option<CodexAuth>,
    pub api_provider: Provider,
    pub api_auth: SharedAuthProvider,
    pub agent_identity_telemetry: Option<AgentIdentityTelemetry>,
    pub strategy: ProviderRequestStrategy,
}

impl ProviderRequestSetup {
    pub fn new(
        auth: Option<CodexAuth>,
        api_provider: Provider,
        api_auth: SharedAuthProvider,
        agent_identity_telemetry: Option<AgentIdentityTelemetry>,
    ) -> Self {
        let strategy = ProviderRequestStrategy::from_provider(
            &api_provider,
            auth.as_ref(),
            api_auth.as_ref(),
            agent_identity_telemetry.as_ref(),
        );
        Self {
            auth,
            api_provider,
            api_auth,
            agent_identity_telemetry,
            strategy,
        }
    }
}

pub(crate) async fn configured_provider_request_setup(
    provider: &ModelProviderInfo,
    auth_manager: Option<&Arc<AuthManager>>,
    scope: ProviderAuthScope,
) -> Result<ProviderRequestSetup> {
    let auth = match auth_manager {
        Some(auth_manager) => auth_manager.auth().await,
        None => None,
    };
    let api_provider = provider.to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))?;
    let resolved_auth = if provider_uses_first_party_auth_path(provider) {
        resolve_provider_auth_for_scope(auth_manager.cloned(), auth.as_ref(), provider, scope)
            .await?
    } else {
        ResolvedProviderAuth::new(resolve_provider_auth(auth.as_ref(), provider)?)
    };
    Ok(ProviderRequestSetup::new(
        auth,
        api_provider,
        resolved_auth.auth,
        resolved_auth.agent_identity_telemetry,
    ))
}

pub(crate) fn provider_uses_first_party_auth_path(provider: &ModelProviderInfo) -> bool {
    provider.requires_openai_auth
        && provider.env_key.is_none()
        && provider.experimental_bearer_token.is_none()
        && provider.auth.is_none()
        && provider.aws.is_none()
}

#[cfg(test)]
#[path = "request_setup_tests.rs"]
mod tests;
