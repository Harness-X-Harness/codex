use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use codex_api::AgentIdentityTelemetry;
use codex_api::ApiError;
use codex_api::ModelsClient;
use codex_api::RequestTelemetry;
use codex_api::ReqwestTransport;
use codex_api::TransportError;
use codex_api::auth_header_telemetry;
use codex_api::map_api_error;
use codex_feedback::FeedbackRequestTags;
use codex_feedback::emit_feedback_request_tags_with_auth_env;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_login::AuthEnvTelemetry;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::collect_auth_env_telemetry;
use codex_login::default_client::create_client_for_route_async;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::cache::ModelsCatalogIdentity;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_otel::TelemetryAuthMode;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;
use codex_response_debug_context::extract_response_debug_context;
use codex_response_debug_context::telemetry_transport_error_message;
use http::HeaderMap;
use sha2::Digest;
use sha2::Sha256;
use tokio::time::timeout;

use crate::auth::agent_identity_telemetry;
use crate::auth::resolve_provider_auth;

pub(crate) const MODELS_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
const MODELS_ENDPOINT: &str = "/models";
const OPENAI_MODELS_AUTHORITY: &str = "openai-compatible-model-catalog";
const OPENAI_MODELS_DECODER_VERSION: &str = "codex-openai-models-v1";

/// Provider-owned OpenAI-compatible `/models` endpoint.
#[derive(Debug)]
pub(crate) struct OpenAiModelsEndpoint {
    request: ModelsEndpointRequest,
}

impl OpenAiModelsEndpoint {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            request: ModelsEndpointRequest::new(provider_info, auth_manager),
        }
    }

    async fn list_models(
        &self,
        client_version: &str,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<(Vec<ModelInfo>, Option<String>)> {
        let _timer =
            codex_otel::start_global_timer("codex.remote_models.fetch_update.duration_ms", &[]);
        timeout(MODELS_REFRESH_TIMEOUT, async {
            let PreparedModelsRequest {
                client,
                request_url,
            } = self
                .request
                .prepare(client_version, http_client_factory)
                .await?;
            let (body, etag) = client
                .fetch_models(request_url, HeaderMap::new())
                .await
                .map_err(map_api_error)?;
            let ModelsResponse { models } = serde_json::from_slice(&body).map_err(|error| {
                map_api_error(ApiError::Stream(format!(
                    "failed to decode models response: {error}"
                )))
            })?;
            Ok((models, etag))
        })
        .await
        .map_err(|_| CodexErr::Timeout)?
    }
}

impl ModelsEndpointClient for OpenAiModelsEndpoint {
    fn catalog_identity(&self) -> ModelsCatalogIdentity {
        ModelsCatalogIdentity::new(
            self.request
                .catalog_authority_identity(OPENAI_MODELS_AUTHORITY),
            OPENAI_MODELS_DECODER_VERSION,
        )
    }

    fn has_command_auth(&self) -> bool {
        self.request.has_command_auth()
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(self.request.uses_codex_backend())
    }

    fn remote_catalog_is_authoritative(&self) -> bool {
        self.request.remote_catalog_is_authoritative()
    }

    fn list_models<'a>(
        &'a self,
        client_version: &'a str,
        http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(OpenAiModelsEndpoint::list_models(
            self,
            client_version,
            http_client_factory,
        ))
    }
}

/// Shared request mechanics for concrete Provider catalog strategies.
#[derive(Debug)]
pub(crate) struct ModelsEndpointRequest {
    provider_info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
    transport_builder: Arc<dyn ModelsTransportBuilder>,
}

impl ModelsEndpointRequest {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            provider_info,
            auth_manager,
            transport_builder: Arc::new(RouteAwareModelsTransportBuilder),
        }
    }

    async fn auth(&self) -> Option<CodexAuth> {
        match self.auth_manager.as_ref() {
            Some(auth_manager) => auth_manager.auth().await,
            None => None,
        }
    }

    pub(crate) async fn uses_codex_backend(&self) -> bool {
        self.auth()
            .await
            .as_ref()
            .is_some_and(CodexAuth::uses_codex_backend)
    }

    pub(crate) async fn prepare(
        &self,
        client_version: &str,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<PreparedModelsRequest> {
        let auth = self.auth().await;
        let auth_mode = auth.as_ref().map(CodexAuth::auth_mode);
        let api_provider = self.provider_info.to_api_provider(auth_mode)?;
        let api_auth = resolve_provider_auth(auth.as_ref(), &self.provider_info)?;
        let request_url =
            ModelsClient::<ReqwestTransport>::request_url(&api_provider, client_version);
        let auth_telemetry = auth_header_telemetry(api_auth.as_ref());
        let agent_identity_telemetry = if let Some(CodexAuth::AgentIdentity(auth)) = auth.as_ref() {
            Some(agent_identity_telemetry(auth))
        } else {
            None
        };
        let request_telemetry: Arc<dyn RequestTelemetry> = Arc::new(ModelsRequestTelemetry {
            auth_mode: auth_mode.map(|mode| TelemetryAuthMode::from(mode).to_string()),
            auth_header_attached: auth_telemetry.attached,
            auth_header_name: auth_telemetry.name,
            agent_identity_telemetry,
            auth_env: self.auth_env(),
        });
        let transport = self
            .transport_builder
            .build(http_client_factory, request_url.clone())
            .await?;
        let client = ModelsClient::new(transport, api_provider, api_auth)
            .with_telemetry(Some(request_telemetry));
        Ok(PreparedModelsRequest {
            client,
            request_url,
        })
    }

    fn auth_env(&self) -> AuthEnvTelemetry {
        let codex_api_key_env_enabled = self
            .auth_manager
            .as_ref()
            .is_some_and(|auth_manager| auth_manager.codex_api_key_env_enabled());
        collect_auth_env_telemetry(&self.provider_info, codex_api_key_env_enabled)
    }

    pub(crate) fn has_command_auth(&self) -> bool {
        self.provider_info.has_command_auth()
    }

    pub(crate) fn remote_catalog_is_authoritative(&self) -> bool {
        self.provider_info.env_key.is_some()
            || self.provider_info.experimental_bearer_token.is_some()
            || self.provider_info.auth.is_some()
    }

    pub(crate) fn catalog_authority_identity(&self, authority: &str) -> String {
        let auth_mode = self
            .auth_manager
            .as_ref()
            .and_then(|manager| manager.auth_cached())
            .as_ref()
            .map(CodexAuth::auth_mode);
        self.catalog_authority_identity_for_auth_mode(authority, auth_mode)
    }

    fn catalog_authority_identity_for_auth_mode(
        &self,
        authority: &str,
        auth_mode: Option<codex_protocol::auth::AuthMode>,
    ) -> String {
        let (base_url, query_params, headers) = match self.provider_info.to_api_provider(auth_mode)
        {
            Ok(provider) => (provider.base_url, provider.query_params, provider.headers),
            Err(_) => (
                self.provider_info.base_url.clone().unwrap_or_default(),
                self.provider_info.query_params.clone(),
                HeaderMap::new(),
            ),
        };
        let mut hasher = Sha256::new();
        hasher.update(b"codex-model-catalog-authority-route-v1\0");
        update_digest_field(&mut hasher, base_url.as_bytes());
        let mut query_params = query_params
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        query_params.sort_unstable();
        hasher.update((query_params.len() as u64).to_be_bytes());
        for (key, value) in query_params {
            update_digest_field(&mut hasher, key.as_bytes());
            update_digest_field(&mut hasher, value.as_bytes());
        }
        let mut headers = headers
            .iter()
            .map(|(name, value)| (name.as_str().as_bytes(), value.as_bytes()))
            .collect::<Vec<_>>();
        headers.sort_unstable();
        hasher.update((headers.len() as u64).to_be_bytes());
        for (name, value) in headers {
            update_digest_field(&mut hasher, name);
            update_digest_field(&mut hasher, value);
        }
        format!("{authority}:route-v1:{:x}", hasher.finalize())
    }
}

fn update_digest_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

pub(crate) struct PreparedModelsRequest {
    pub(crate) client: ModelsClient<ReqwestTransport>,
    pub(crate) request_url: String,
}

type ModelsTransportFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<ReqwestTransport>> + Send + 'a>>;

/// Builds the concrete transport selected for one models request.
///
/// Implementations must honor the supplied request-time client factory and exact request URL.
trait ModelsTransportBuilder: fmt::Debug + Send + Sync {
    fn build(
        &self,
        http_client_factory: HttpClientFactory,
        request_url: String,
    ) -> ModelsTransportFuture<'_>;
}

#[derive(Debug)]
struct RouteAwareModelsTransportBuilder;

impl ModelsTransportBuilder for RouteAwareModelsTransportBuilder {
    fn build(
        &self,
        http_client_factory: HttpClientFactory,
        request_url: String,
    ) -> ModelsTransportFuture<'_> {
        Box::pin(async move {
            create_client_for_route_async(http_client_factory, request_url, ClientRouteClass::Api)
                .await
                .map(ReqwestTransport::from_http_client)
        })
    }
}

#[derive(Clone)]
struct ModelsRequestTelemetry {
    auth_mode: Option<String>,
    auth_header_attached: bool,
    auth_header_name: Option<&'static str>,
    agent_identity_telemetry: Option<AgentIdentityTelemetry>,
    auth_env: AuthEnvTelemetry,
}

impl RequestTelemetry for ModelsRequestTelemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let success = status.is_some_and(|code| code.is_success()) && error.is_none();
        let error_message = error.map(telemetry_transport_error_message);
        let response_debug = error
            .map(extract_response_debug_context)
            .unwrap_or_default();
        let status = status.map(|status| status.as_u16());
        tracing::event!(
            target: "codex_otel.log_only",
            tracing::Level::INFO,
            event.name = "codex.api_request",
            duration_ms = %duration.as_millis(),
            http.response.status_code = status,
            success = success,
            error.message = error_message.as_deref(),
            attempt = attempt,
            endpoint = MODELS_ENDPOINT,
            auth.header_attached = self.auth_header_attached,
            auth.header_name = self.auth_header_name,
            auth.env_openai_api_key_present = self.auth_env.openai_api_key_env_present,
            auth.env_codex_api_key_present = self.auth_env.codex_api_key_env_present,
            auth.env_codex_api_key_enabled = self.auth_env.codex_api_key_env_enabled,
            auth.env_provider_key_name = self.auth_env.provider_env_key_name.as_deref(),
            auth.env_provider_key_present = self.auth_env.provider_env_key_present,
            auth.env_refresh_token_url_override_present = self.auth_env.refresh_token_url_override_present,
            auth.request_id = response_debug.request_id.as_deref(),
            auth.cf_ray = response_debug.cf_ray.as_deref(),
            auth.error = response_debug.auth_error.as_deref(),
            auth.error_code = response_debug.auth_error_code.as_deref(),
            auth.mode = self.auth_mode.as_deref(),
            auth.agent_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.agent_id.as_str()),
            auth.task_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.task_id.as_str()),
        );
        tracing::event!(
            target: "codex_otel.trace_safe",
            tracing::Level::INFO,
            event.name = "codex.api_request",
            duration_ms = %duration.as_millis(),
            http.response.status_code = status,
            success = success,
            error.message = error_message.as_deref(),
            attempt = attempt,
            endpoint = MODELS_ENDPOINT,
            auth.header_attached = self.auth_header_attached,
            auth.header_name = self.auth_header_name,
            auth.env_openai_api_key_present = self.auth_env.openai_api_key_env_present,
            auth.env_codex_api_key_present = self.auth_env.codex_api_key_env_present,
            auth.env_codex_api_key_enabled = self.auth_env.codex_api_key_env_enabled,
            auth.env_provider_key_name = self.auth_env.provider_env_key_name.as_deref(),
            auth.env_provider_key_present = self.auth_env.provider_env_key_present,
            auth.env_refresh_token_url_override_present = self.auth_env.refresh_token_url_override_present,
            auth.request_id = response_debug.request_id.as_deref(),
            auth.cf_ray = response_debug.cf_ray.as_deref(),
            auth.error = response_debug.auth_error.as_deref(),
            auth.error_code = response_debug.auth_error_code.as_deref(),
            auth.mode = self.auth_mode.as_deref(),
            auth.agent_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.agent_id.as_str()),
            auth.task_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.task_id.as_str()),
        );
        emit_feedback_request_tags_with_auth_env(
            &FeedbackRequestTags {
                endpoint: MODELS_ENDPOINT,
                auth_header_attached: self.auth_header_attached,
                auth_header_name: self.auth_header_name,
                auth_mode: self.auth_mode.as_deref(),
                auth_retry_after_unauthorized: None,
                auth_recovery_mode: None,
                auth_recovery_phase: None,
                auth_connection_reused: None,
                auth_request_id: response_debug.request_id.as_deref(),
                auth_cf_ray: response_debug.cf_ray.as_deref(),
                auth_error: response_debug.auth_error.as_deref(),
                auth_error_code: response_debug.auth_error_code.as_deref(),
                auth_recovery_followup_success: None,
                auth_recovery_followup_status: None,
            },
            &self.auth_env,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::num::NonZeroU64;
    use std::sync::Mutex;

    use super::*;
    use codex_http_client::OutboundProxyPolicy;
    use codex_login::default_client::create_client;
    use codex_protocol::config_types::ModelProviderAuthInfo;
    use codex_protocol::openai_models::ModelsResponse;
    use pretty_assertions::assert_eq;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::matchers::query_param;

    #[derive(Debug)]
    struct RecordingTransportBuilder {
        observed_request: Arc<Mutex<Option<(OutboundProxyPolicy, String)>>>,
    }

    impl ModelsTransportBuilder for RecordingTransportBuilder {
        fn build(
            &self,
            http_client_factory: HttpClientFactory,
            request_url: String,
        ) -> ModelsTransportFuture<'_> {
            let observed_request = Arc::clone(&self.observed_request);
            Box::pin(async move {
                *observed_request
                    .lock()
                    .expect("observed request lock should not be poisoned") =
                    Some((http_client_factory.outbound_proxy_policy(), request_url));
                Ok(ReqwestTransport::from_http_client(create_client()))
            })
        }
    }

    fn provider_info_with_command_auth() -> ModelProviderInfo {
        ModelProviderInfo {
            auth: Some(ModelProviderAuthInfo {
                command: "print-token".to_string(),
                args: Vec::new(),
                timeout_ms: NonZeroU64::new(5_000).expect("timeout should be non-zero"),
                refresh_interval_ms: 300_000,
                cwd: std::env::current_dir()
                    .expect("current dir should be available")
                    .try_into()
                    .expect("current dir should be absolute"),
            }),
            requires_openai_auth: false,
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        }
    }

    #[test]
    fn command_auth_provider_reports_command_auth_without_cached_auth() {
        let endpoint = OpenAiModelsEndpoint::new(
            provider_info_with_command_auth(),
            /*auth_manager*/ None,
        );

        assert!(endpoint.has_command_auth());
    }

    #[test]
    fn provider_without_command_auth_reports_no_command_auth() {
        let endpoint = OpenAiModelsEndpoint::new(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            /*auth_manager*/ None,
        );

        assert!(!endpoint.has_command_auth());
    }

    #[test]
    fn catalog_identity_partitions_routes_without_exposing_the_url() {
        let first_url = "https://first.example.test/private-token";
        let second_url = "https://second.example.test/private-token";
        let first = OpenAiModelsEndpoint::new(
            ModelProviderInfo::create_openai_provider(Some(first_url.to_string())),
            /*auth_manager*/ None,
        );
        let second = OpenAiModelsEndpoint::new(
            ModelProviderInfo::create_openai_provider(Some(second_url.to_string())),
            /*auth_manager*/ None,
        );

        assert_ne!(first.catalog_identity(), second.catalog_identity());
        assert!(!first.catalog_identity().authority.contains(first_url));
    }

    #[test]
    fn catalog_identity_partitions_header_selected_authorities_without_exposing_values() {
        let first_tenant = "private-project-one";
        let second_tenant = "private-project-two";
        let mut first_info = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        first_info.http_headers = Some(HashMap::from([(
            "OpenAI-Project".to_string(),
            first_tenant.to_string(),
        )]));
        let mut second_info = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        second_info.http_headers = Some(HashMap::from([(
            "OpenAI-Project".to_string(),
            second_tenant.to_string(),
        )]));
        let first = OpenAiModelsEndpoint::new(first_info, /*auth_manager*/ None);
        let second = OpenAiModelsEndpoint::new(second_info, /*auth_manager*/ None);

        assert_ne!(first.catalog_identity(), second.catalog_identity());
        assert!(!first.catalog_identity().authority.contains(first_tenant));
    }

    #[test]
    fn catalog_identity_uses_the_effective_auth_route() {
        let mut provider_info = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        provider_info.http_headers = None;
        provider_info.env_http_headers = None;
        let request = ModelsEndpointRequest::new(provider_info, /*auth_manager*/ None);

        let api = request.catalog_authority_identity_for_auth_mode(
            OPENAI_MODELS_AUTHORITY,
            /*auth_mode*/ None,
        );
        let chatgpt = request.catalog_authority_identity_for_auth_mode(
            OPENAI_MODELS_AUTHORITY,
            Some(codex_protocol::auth::AuthMode::Chatgpt),
        );

        assert_ne!(api, chatgpt);
        assert_eq!(
            api,
            "openai-compatible-model-catalog:route-v1:\
             500bbe9d5a630e6bdc661015983010e51a7dfcd06fb7cce0585a2296b951b650"
        );
    }

    #[tokio::test]
    async fn model_request_uses_request_time_proxy_policy_and_exact_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(query_param("client_version", "0.0.0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ModelsResponse { models: Vec::new() }),
            )
            .expect(1)
            .mount(&server)
            .await;

        let observed_request = Arc::new(Mutex::new(None));
        let endpoint = OpenAiModelsEndpoint {
            request: ModelsEndpointRequest {
                provider_info: ModelProviderInfo::create_openai_provider(Some(server.uri())),
                auth_manager: None,
                transport_builder: Arc::new(RecordingTransportBuilder {
                    observed_request: Arc::clone(&observed_request),
                }),
            },
        };

        endpoint
            .list_models(
                "0.0.0",
                HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy),
            )
            .await
            .expect("models request should succeed");

        assert_eq!(
            *observed_request
                .lock()
                .expect("observed request lock should not be poisoned"),
            Some((
                OutboundProxyPolicy::RespectSystemProxy,
                format!("{}/models?client_version=0.0.0", server.uri()),
            ))
        );
    }
}
