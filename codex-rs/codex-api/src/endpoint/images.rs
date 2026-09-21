use crate::auth::SharedAuthProvider;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::grok_images;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageResponse;
use crate::provider::ApiDialect;
use crate::provider::Provider;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use http::HeaderMap;
use http::Method;
use serde_json::Value;
use serde_json::to_value;
use std::sync::Arc;

const X_CODEX_IMAGEGEN_REQUEST_ID_HEADER: &str = "x-codex-imagegen-request-id";

pub struct ImagesClient<T: HttpTransport> {
    session: EndpointSession<T>,
}

impl<T: HttpTransport> ImagesClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(self, request: Option<Arc<dyn RequestTelemetry>>) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
        }
    }

    pub async fn generate(
        &self,
        request: &ImageGenerationRequest,
        extra_headers: HeaderMap,
    ) -> Result<(ImageResponse, Option<String>), ApiError> {
        let body = match self.session.provider().dialect {
            ApiDialect::OpenAi => to_value(request).map_err(|error| {
                ApiError::Stream(format!(
                    "failed to encode image generation request: {error}"
                ))
            }),
            ApiDialect::Grok => Ok(grok_images::generation_body(request)),
        };
        self.post_image_request(
            "images/generations",
            body,
            extra_headers,
            "image generation",
        )
        .await
    }

    pub async fn edit(
        &self,
        request: &ImageEditRequest,
        extra_headers: HeaderMap,
    ) -> Result<(ImageResponse, Option<String>), ApiError> {
        let body = match self.session.provider().dialect {
            ApiDialect::OpenAi => to_value(request).map_err(|error| {
                ApiError::Stream(format!("failed to encode image edit request: {error}"))
            }),
            ApiDialect::Grok => grok_images::edit_body(request),
        };
        self.post_image_request("images/edits", body, extra_headers, "image edit")
            .await
    }

    async fn post_image_request(
        &self,
        path: &str,
        body: Result<Value, ApiError>,
        extra_headers: HeaderMap,
        operation: &str,
    ) -> Result<(ImageResponse, Option<String>), ApiError> {
        let body = body?;
        let resp = self
            .session
            .execute(Method::POST, path, extra_headers, Some(body))
            .await?;
        let imagegen_request_id = resp
            .headers
            .get(X_CODEX_IMAGEGEN_REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|request_id| !request_id.is_empty())
            .map(str::to_string);
        let response = match self.session.provider().dialect {
            ApiDialect::OpenAi => serde_json::from_slice(&resp.body).map_err(|error| {
                ApiError::Stream(format!("failed to decode {operation} response: {error}"))
            }),
            ApiDialect::Grok => grok_images::decode_response(&resp.body, operation),
        }?;
        Ok((response, imagegen_request_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthProvider;
    use crate::images::ImageBackground;
    use crate::images::ImageData;
    use crate::images::ImageQuality;
    use crate::images::ImageUrl;
    use crate::provider::RetryConfig;
    use codex_client::Request;
    use codex_client::RequestBody;
    use codex_client::Response;
    use codex_client::StreamResponse;
    use codex_client::TransportError;
    use http::StatusCode;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct DummyAuth;

    impl AuthProvider for DummyAuth {
        fn add_auth_headers(&self, _headers: &mut HeaderMap) {}
    }

    #[derive(Clone)]
    struct CapturingTransport {
        last_request: Arc<Mutex<Option<Request>>>,
        response_body: Arc<Vec<u8>>,
        response_headers: HeaderMap,
    }

    impl CapturingTransport {
        fn new(response_body: Vec<u8>) -> Self {
            Self::with_response_headers(response_body, HeaderMap::new())
        }

        fn with_response_headers(response_body: Vec<u8>, response_headers: HeaderMap) -> Self {
            Self {
                last_request: Arc::new(Mutex::new(None)),
                response_body: Arc::new(response_body),
                response_headers,
            }
        }
    }

    impl HttpTransport for CapturingTransport {
        async fn execute(&self, req: Request) -> Result<Response, TransportError> {
            *self.last_request.lock().expect("lock request store") = Some(req);
            Ok(Response {
                status: StatusCode::OK,
                headers: self.response_headers.clone(),
                body: self.response_body.as_ref().clone().into(),
            })
        }

        async fn stream(&self, _req: Request) -> Result<StreamResponse, TransportError> {
            Err(TransportError::Build("stream should not run".to_string()))
        }
    }

    fn provider_with_dialect(dialect: crate::ApiDialect) -> Provider {
        Provider {
            name: "test".to_string(),
            base_url: "https://example.com/api/codex".to_string(),
            query_params: None,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(1),
            dialect,
            x_search: None,
        }
    }

    fn provider() -> Provider {
        provider_with_dialect(crate::ApiDialect::OpenAi)
    }

    fn grok_provider() -> Provider {
        let mut provider = provider_with_dialect(crate::ApiDialect::Grok);
        provider.name = "openai".to_string();
        provider
    }

    fn response_body() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "created": 1778832973u64,
            "background": "opaque",
            "data": [{"b64_json": "REDACT"}],
            "output_format": "png",
            "quality": "medium",
            "size": "1024x1536",
            "usage": {
                "input_tokens": 1474,
                "input_tokens_details": {
                    "image_tokens": 1457,
                    "text_tokens": 17,
                },
                "output_tokens": 1372,
                "output_tokens_details": {
                    "image_tokens": 1372,
                    "text_tokens": 0,
                },
                "total_tokens": 2846,
            }
        }))
        .expect("serialize response")
    }

    fn expected_response() -> ImageResponse {
        ImageResponse {
            created: 1778832973,
            background: Some(ImageBackground::Opaque),
            data: vec![ImageData {
                b64_json: "REDACT".to_string(),
                generation_id: None,
            }],
            quality: Some(ImageQuality::Medium),
            size: Some("1024x1536".to_string()),
        }
    }

    fn captured_request(transport: &CapturingTransport) -> Request {
        transport
            .last_request
            .lock()
            .expect("lock request store")
            .clone()
            .expect("request should be captured")
    }

    #[tokio::test]
    async fn generate_posts_typed_request_and_parses_image_response() {
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            X_CODEX_IMAGEGEN_REQUEST_ID_HEADER,
            http::HeaderValue::from_static("req-imagegen-123"),
        );
        response_headers.insert(
            "x-request-id",
            http::HeaderValue::from_static("req-outer-456"),
        );
        let transport =
            CapturingTransport::with_response_headers(response_body(), response_headers);
        let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth));

        let (response, imagegen_request_id) = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "a red fox in a field".to_string(),
                    background: Some(ImageBackground::Opaque),
                    model: "gpt-image-1.5".to_string(),
                    n: None,
                    quality: Some(ImageQuality::Medium),
                    size: Some("1024x1536".to_string()),
                },
                HeaderMap::new(),
            )
            .await
            .expect("image generation request should succeed");

        assert_eq!(response, expected_response());
        assert_eq!(imagegen_request_id.as_deref(), Some("req-imagegen-123"));

        let request = captured_request(&transport);
        assert_eq!(
            request.url,
            "https://example.com/api/codex/images/generations"
        );
        assert_eq!(
            request.body.as_ref().and_then(RequestBody::json),
            Some(&json!({
                "prompt": "a red fox in a field",
                "background": "opaque",
                "model": "gpt-image-1.5",
                "quality": "medium",
                "size": "1024x1536",
            }))
        );
    }

    #[tokio::test]
    async fn preserves_distinct_generation_ids_on_image_data() {
        let body = serde_json::to_vec(&json!({
            "created": 1,
            "data": [
                {"b64_json": "first", "generation_id": "gen-first"},
                {"b64_json": "second", "generation_id": "gen-second"},
                {"b64_json": "legacy"}
            ]
        }))
        .expect("serialize response");
        let transport = CapturingTransport::new(body);
        let client = ImagesClient::new(transport, provider(), Arc::new(DummyAuth));

        let (response, _) = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "test".to_string(),
                    background: None,
                    model: "gpt-image-2".to_string(),
                    n: None,
                    quality: None,
                    size: None,
                },
                HeaderMap::new(),
            )
            .await
            .expect("image response should parse");

        assert_eq!(
            response
                .data
                .iter()
                .map(|image| image.generation_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("gen-first"), Some("gen-second"), None]
        );
    }

    #[tokio::test]
    async fn edit_posts_typed_request_and_parses_image_response() {
        let transport = CapturingTransport::new(response_body());
        let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth));

        let (response, imagegen_request_id) = client
            .edit(
                &ImageEditRequest {
                    images: vec![ImageUrl {
                        image_url: "data:image/png;base64,Zm9v".to_string(),
                    }],
                    prompt: "add a red hat".to_string(),
                    background: None,
                    model: "gpt-image-1.5".to_string(),
                    n: None,
                    quality: None,
                    size: None,
                },
                HeaderMap::new(),
            )
            .await
            .expect("image edit request should succeed");

        assert_eq!(response, expected_response());
        assert_eq!(imagegen_request_id, None);

        let request = captured_request(&transport);
        assert_eq!(request.url, "https://example.com/api/codex/images/edits");
        assert_eq!(
            request.body.as_ref().and_then(RequestBody::json),
            Some(&json!({
                "images": [{"image_url": "data:image/png;base64,Zm9v"}],
                "prompt": "add a red hat",
                "model": "gpt-image-1.5",
            }))
        );
    }

    #[tokio::test]
    async fn image_response_requires_image_data() {
        let transport = CapturingTransport::new(
            serde_json::to_vec(&json!({"created": 1778832973u64})).expect("serialize response"),
        );
        let client = ImagesClient::new(transport, provider(), Arc::new(DummyAuth));

        let error = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "a red fox in a field".to_string(),
                    background: None,
                    model: "gpt-image-1.5".to_string(),
                    n: None,
                    quality: None,
                    size: None,
                },
                HeaderMap::new(),
            )
            .await
            .expect_err("image response without data should fail");

        let ApiError::Stream(message) = error else {
            panic!("expected image response decode error");
        };
        assert!(
            message.starts_with("failed to decode image generation response: missing field `data`"),
            "{message}"
        );
    }

    fn grok_response_body() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "data": [{"b64_json": "REDACT", "mime_type": "image/jpeg"}]
        }))
        .expect("serialize Grok response")
    }

    #[tokio::test]
    async fn stock_image_response_still_requires_created() {
        let transport = CapturingTransport::new(grok_response_body());
        let client = ImagesClient::new(transport, provider(), Arc::new(DummyAuth));

        let error = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "stock".to_string(),
                    background: None,
                    model: "gpt-image-2".to_string(),
                    n: None,
                    quality: None,
                    size: None,
                },
                HeaderMap::new(),
            )
            .await
            .expect_err("stock response without created should fail");

        assert!(error.to_string().contains("missing field `created`"));
    }

    #[tokio::test]
    async fn grok_generation_uses_dialect_not_display_name() {
        let transport = CapturingTransport::new(grok_response_body());
        let client = ImagesClient::new(transport.clone(), grok_provider(), Arc::new(DummyAuth));

        let (response, _) = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "draw a fox".to_string(),
                    background: Some(ImageBackground::Transparent),
                    model: "gpt-image-2".to_string(),
                    n: Some(2),
                    quality: Some(ImageQuality::High),
                    size: Some("1024x1024".to_string()),
                },
                HeaderMap::new(),
            )
            .await
            .expect("Grok image generation should decode");

        assert_eq!(response.created, 0);
        assert_eq!(
            captured_request(&transport)
                .body
                .as_ref()
                .and_then(RequestBody::json),
            Some(&json!({
                "model": "grok-imagine-image-2.0",
                "prompt": "draw a fox",
                "response_format": "b64_json",
            }))
        );
    }

    #[tokio::test]
    async fn named_grok_with_openai_dialect_stays_stock() {
        let mut provider = provider();
        provider.name = "Grok".to_string();
        let transport = CapturingTransport::new(response_body());
        let client = ImagesClient::new(transport.clone(), provider, Arc::new(DummyAuth));

        client
            .generate(
                &ImageGenerationRequest {
                    prompt: "a red fox in a field".to_string(),
                    background: Some(ImageBackground::Opaque),
                    model: "gpt-image-1.5".to_string(),
                    n: None,
                    quality: Some(ImageQuality::Medium),
                    size: Some("1024x1536".to_string()),
                },
                HeaderMap::new(),
            )
            .await
            .expect("named Grok with OpenAI dialect should stay stock");

        assert_eq!(
            captured_request(&transport)
                .body
                .as_ref()
                .and_then(RequestBody::json),
            Some(&json!({
                "prompt": "a red fox in a field",
                "background": "opaque",
                "model": "gpt-image-1.5",
                "quality": "medium",
                "size": "1024x1536",
            }))
        );
    }

    #[tokio::test]
    async fn grok_edit_rejects_unsupported_cardinality_before_transport() {
        let transport = CapturingTransport::new(grok_response_body());
        let client = ImagesClient::new(transport.clone(), grok_provider(), Arc::new(DummyAuth));
        client
            .edit(
                &ImageEditRequest {
                    images: Vec::new(),
                    prompt: "edit".to_string(),
                    background: None,
                    model: "gpt-image-2".to_string(),
                    n: None,
                    quality: None,
                    size: None,
                },
                HeaderMap::new(),
            )
            .await
            .expect_err("unsupported image count should fail");
        assert!(
            transport
                .last_request
                .lock()
                .expect("lock request")
                .is_none()
        );
    }
}
