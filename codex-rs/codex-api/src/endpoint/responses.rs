use crate::auth::SharedAuthProvider;
use crate::common::ResponseStream;
use crate::common::ResponsesApiRequest;
use crate::common::SearchActivityAdmission;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::ApiDialect;
use crate::provider::Provider;
use crate::requests::Compression;
use crate::requests::headers::build_session_headers;
use crate::requests::headers::insert_header;
use crate::requests::headers::subagent_header;
use crate::sse::responses::spawn_response_stream_with_search;
use crate::telemetry::SseTelemetry;
use codex_client::EncodedJsonBody;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_client::RequestTelemetry;
use codex_protocol::grok::GrokXSearchOptions;
use codex_protocol::protocol::SessionSource;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;
use tracing::instrument;

pub struct ResponsesClient<T: HttpTransport> {
    session: EndpointSession<T>,
    dialect: ApiDialect,
    grok_x_search: Option<GrokXSearchOptions>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

#[derive(Default)]
pub struct ResponsesOptions {
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub session_source: Option<SessionSource>,
    pub extra_headers: HeaderMap,
    pub compression: Compression,
    pub turn_state: Option<Arc<OnceLock<String>>>,
}

impl<T: HttpTransport> ResponsesClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            dialect: ApiDialect::OpenAi,
            grok_x_search: None,
            sse_telemetry: None,
        }
    }

    /// Select typed HTTP semantics independently of name/destination. Grok rejects
    /// unsupported history/tools/media and raw JSON before transport; OpenAI is default.
    pub fn with_dialect(mut self, dialect: ApiDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Configure Grok's default X Search dates without changing transport identity.
    pub fn with_grok_x_search(mut self, options: Option<GrokXSearchOptions>) -> Self {
        self.grok_x_search = options;
        self
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            dialect: self.dialect,
            grok_x_search: self.grok_x_search,
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "responses.stream_request",
        level = "info",
        skip_all,
        fields(
            transport = "responses_http",
            http.method = "POST",
            api.path = "/responses"
        )
    )]
    pub async fn stream_request(
        &self,
        request: ResponsesApiRequest,
        options: ResponsesOptions,
    ) -> Result<ResponseStream, ApiError> {
        validate_grok_x_search(self.dialect, self.grok_x_search.as_ref())?;
        let ResponsesOptions {
            session_id,
            thread_id,
            session_source,
            extra_headers,
            compression,
            turn_state,
        } = options;
        let mut search_activity = SearchActivityAdmission::Disabled;
        let body = match self.dialect {
            ApiDialect::OpenAi => EncodedJsonBody::encode(&request),
            ApiDialect::Grok => {
                let projected =
                    crate::grok_request::build_with_search(&request, self.grok_x_search.as_ref())?;
                // Inspect the admitted wire request, not a provider name or an
                // unvalidated incoming status. Projection remains the policy owner.
                if let Some(tools) = projected["tools"].as_array() {
                    search_activity = match (
                        tools.iter().any(|tool| tool["type"] == "web_search"),
                        tools.iter().any(|tool| tool["type"] == "x_search"),
                    ) {
                        (false, false) => SearchActivityAdmission::Disabled,
                        (true, false) => SearchActivityAdmission::Web,
                        (false, true) => SearchActivityAdmission::X,
                        (true, true) => SearchActivityAdmission::WebAndX,
                    };
                }
                EncodedJsonBody::encode(&projected)
            }
        }
        .map_err(|e| ApiError::Stream(format!("failed to encode responses request: {e}")))?;

        let mut headers = extra_headers;
        if let Some(ref thread_id) = thread_id {
            insert_header(&mut headers, "x-client-request-id", thread_id);
        }
        headers.extend(build_session_headers(session_id, thread_id));
        if let Some(subagent) = subagent_header(&session_source) {
            insert_header(&mut headers, "x-openai-subagent", &subagent);
        }

        self.stream_encoded(body, headers, compression, turn_state, search_activity)
            .await
    }

    #[instrument(
        name = "responses.stream",
        level = "info",
        skip_all,
        fields(
            transport = "responses_http",
            http.method = "POST",
            api.path = "/responses",
            turn.has_state = turn_state.is_some()
        )
    )]
    pub async fn stream(
        &self,
        body: Value,
        extra_headers: HeaderMap,
        compression: Compression,
        turn_state: Option<Arc<OnceLock<String>>>,
    ) -> Result<ResponseStream, ApiError> {
        validate_grok_x_search(self.dialect, self.grok_x_search.as_ref())?;
        if self.dialect == ApiDialect::Grok {
            return Err(ApiError::Stream(
                "Grok requires typed stream_request; raw JSON bypasses projection".into(),
            ));
        }
        let body = EncodedJsonBody::encode(&body)
            .map_err(|e| ApiError::Stream(format!("failed to encode responses request: {e}")))?;
        self.stream_encoded(
            body,
            extra_headers,
            compression,
            turn_state,
            SearchActivityAdmission::Disabled,
        )
        .await
    }

    async fn stream_encoded(
        &self,
        body: EncodedJsonBody,
        extra_headers: HeaderMap,
        compression: Compression,
        turn_state: Option<Arc<OnceLock<String>>>,
        search_activity: SearchActivityAdmission,
    ) -> Result<ResponseStream, ApiError> {
        let request_compression = match compression {
            Compression::None => RequestCompression::None,
            Compression::Zstd => RequestCompression::Zstd,
        };

        let stream_response = self
            .session
            .stream_encoded_json_with(
                Method::POST,
                "/responses",
                extra_headers,
                Some(body),
                |req| {
                    req.headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("text/event-stream"),
                    );
                    req.compression = request_compression;
                },
            )
            .await?;

        Ok(spawn_response_stream_with_search(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
            turn_state,
            self.dialect,
            search_activity,
        ))
    }
}

fn validate_grok_x_search(
    dialect: ApiDialect,
    options: Option<&GrokXSearchOptions>,
) -> Result<(), ApiError> {
    if options.is_some() && dialect != ApiDialect::Grok {
        return Err(ApiError::Stream(
            "X Search options require the Grok Responses dialect".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
