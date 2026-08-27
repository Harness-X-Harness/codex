use crate::auth::SharedAuthProvider;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageResponse;
use crate::images::ImagesDialect;
use crate::provider::Provider;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use http::HeaderMap;
use http::Method;
use serde_json::to_value;
use std::sync::Arc;

pub struct ImagesClient<T: HttpTransport> {
    session: EndpointSession<T>,
    dialect: ImagesDialect,
}

impl<T: HttpTransport> ImagesClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            dialect: ImagesDialect::OpenAi,
        }
    }

    pub fn with_dialect(mut self, dialect: ImagesDialect) -> Self {
        self.dialect = dialect;
        self
    }

    pub fn with_telemetry(self, request: Option<Arc<dyn RequestTelemetry>>) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            dialect: self.dialect,
        }
    }

    pub async fn generate(
        &self,
        request: &ImageGenerationRequest,
        extra_headers: HeaderMap,
    ) -> Result<ImageResponse, ApiError> {
        let body = match self.dialect {
            ImagesDialect::OpenAi => {
                to_value(request).map_err(|error| ApiError::Stream(error.to_string()))
            }
            ImagesDialect::Grok => Ok(serde_json::json!({
                "model": request.model.as_str(),
                "prompt": request.prompt.as_str(),
                "response_format": "b64_json",
            })),
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
    ) -> Result<ImageResponse, ApiError> {
        let body = match self.dialect {
            ImagesDialect::OpenAi => {
                to_value(request).map_err(|error| ApiError::Stream(error.to_string()))
            }
            ImagesDialect::Grok => grok_edit_body(request),
        };
        self.post_image_request("images/edits", body, extra_headers, "image edit")
            .await
    }

    async fn post_image_request(
        &self,
        path: &str,
        body: Result<serde_json::Value, ApiError>,
        extra_headers: HeaderMap,
        operation: &str,
    ) -> Result<ImageResponse, ApiError> {
        let body = body?;
        let resp = self
            .session
            .execute(Method::POST, path, extra_headers, Some(body))
            .await?;
        let response: ImageResponse = serde_json::from_slice(&resp.body)
            .map_err(|e| ApiError::Stream(format!("failed to decode {operation} response: {e}")))?;
        if self.dialect == ImagesDialect::OpenAi && response.created.is_none() {
            return Err(ApiError::Stream(format!(
                "failed to decode {operation} response: missing field `created`"
            )));
        }
        Ok(response)
    }
}

fn grok_edit_body(request: &ImageEditRequest) -> Result<serde_json::Value, ApiError> {
    if !(1..=3).contains(&request.images.len()) {
        return Err(ApiError::Stream(
            "Grok image edits require between 1 and 3 images".to_string(),
        ));
    }
    let images = request
        .images
        .iter()
        .map(|image| {
            serde_json::json!({
                "type": "image_url",
                "url": image.image_url.as_str(),
            })
        })
        .collect::<Vec<_>>();
    let image_field = if images.len() == 1 { "image" } else { "images" };
    let mut body = serde_json::json!({
        "model": request.model.as_str(),
        "prompt": request.prompt.as_str(),
        "response_format": "b64_json",
    });
    if let Some(object) = body.as_object_mut() {
        object.insert(
            image_field.to_string(),
            if images.len() == 1 {
                images[0].clone()
            } else {
                serde_json::Value::Array(images)
            },
        );
    }
    Ok(body)
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
    }

    impl CapturingTransport {
        fn new(response_body: Vec<u8>) -> Self {
            Self {
                last_request: Arc::new(Mutex::new(None)),
                response_body: Arc::new(response_body),
            }
        }
    }

    impl HttpTransport for CapturingTransport {
        async fn execute(&self, req: Request) -> Result<Response, TransportError> {
            *self.last_request.lock().expect("lock request store") = Some(req);
            Ok(Response {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: self.response_body.as_ref().clone().into(),
            })
        }

        async fn stream(&self, _req: Request) -> Result<StreamResponse, TransportError> {
            Err(TransportError::Build("stream should not run".to_string()))
        }
    }

    fn provider() -> Provider {
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
            responses_dialect: crate::provider::ResponsesDialect::OpenAi,
        }
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
            created: Some(1778832973),
            background: Some(ImageBackground::Opaque),
            data: vec![ImageData {
                b64_json: "REDACT".to_string(),
                mime_type: None,
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
        let transport = CapturingTransport::new(response_body());
        let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth));

        let response = client
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
    async fn edit_posts_typed_request_and_parses_image_response() {
        let transport = CapturingTransport::new(response_body());
        let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth));

        let response = client
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

    #[tokio::test]
    async fn grok_generation_uses_verified_wire_and_accepts_mime_without_created() {
        let transport = CapturingTransport::new(
            serde_json::to_vec(&json!({
                "data": [{"b64_json": "REDACT", "mime_type": "image/jpeg"}]
            }))
            .expect("serialize response"),
        );
        let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth))
            .with_dialect(ImagesDialect::Grok);
        let response = client
            .generate(
                &ImageGenerationRequest {
                    prompt: "draw".to_string(),
                    background: Some(ImageBackground::Auto),
                    model: "grok-imagine-image-2.0".to_string(),
                    n: None,
                    quality: Some(ImageQuality::Auto),
                    size: Some("auto".to_string()),
                },
                HeaderMap::new(),
            )
            .await
            .expect("grok response should decode");

        assert_eq!(
            response,
            ImageResponse {
                created: None,
                data: vec![ImageData {
                    b64_json: "REDACT".to_string(),
                    mime_type: Some("image/jpeg".to_string()),
                }],
                background: None,
                quality: None,
                size: None,
            }
        );
        assert_eq!(
            captured_request(&transport)
                .body
                .as_ref()
                .and_then(RequestBody::json),
            Some(&json!({
                "model": "grok-imagine-image-2.0",
                "prompt": "draw",
                "response_format": "b64_json"
            }))
        );
    }

    #[tokio::test]
    async fn grok_edit_projects_single_and_multiple_image_shapes() {
        for (images, expected) in [
            (
                vec![ImageUrl {
                    image_url: "one".to_string(),
                }],
                json!({
                    "model": "grok-imagine-image-2.0",
                    "prompt": "edit",
                    "response_format": "b64_json",
                    "image": {"type":"image_url","url":"one"}
                }),
            ),
            (
                vec![
                    ImageUrl {
                        image_url: "one".to_string(),
                    },
                    ImageUrl {
                        image_url: "two".to_string(),
                    },
                ],
                json!({
                    "model": "grok-imagine-image-2.0",
                    "prompt": "edit",
                    "response_format": "b64_json",
                    "images":[
                        {"type":"image_url","url":"one"},
                        {"type":"image_url","url":"two"}
                    ]
                }),
            ),
        ] {
            let transport = CapturingTransport::new(response_body());
            let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth))
                .with_dialect(ImagesDialect::Grok);
            client
                .edit(
                    &ImageEditRequest {
                        images,
                        prompt: "edit".to_string(),
                        background: None,
                        model: "grok-imagine-image-2.0".to_string(),
                        n: None,
                        quality: None,
                        size: None,
                    },
                    HeaderMap::new(),
                )
                .await
                .expect("edit should work");
            let body = captured_request(&transport)
                .body
                .as_ref()
                .and_then(RequestBody::json)
                .cloned()
                .expect("json body");
            assert_eq!(body, expected);
        }
    }

    #[tokio::test]
    async fn grok_edit_rejects_unsupported_cardinality_before_transport() {
        for count in [0, 4] {
            let transport = CapturingTransport::new(response_body());
            let client = ImagesClient::new(transport.clone(), provider(), Arc::new(DummyAuth))
                .with_dialect(ImagesDialect::Grok);
            let error = client
                .edit(
                    &ImageEditRequest {
                        images: (0..count)
                            .map(|index| ImageUrl {
                                image_url: format!("image-{index}"),
                            })
                            .collect(),
                        prompt: "edit".to_string(),
                        background: None,
                        model: "grok-imagine-image-2.0".to_string(),
                        n: None,
                        quality: None,
                        size: None,
                    },
                    HeaderMap::new(),
                )
                .await
                .expect_err("unsupported image count should fail");
            assert!(error.to_string().contains("between 1 and 3"));
            assert!(
                transport
                    .last_request
                    .lock()
                    .expect("lock request")
                    .is_none()
            );
        }
    }
}
