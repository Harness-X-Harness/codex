use crate::ApiDialect;
use crate::ApiError;
use crate::AuthProvider;
use crate::GROK_IMAGE_MODEL;
use crate::ImageBackground;
use crate::ImageData;
use crate::ImageEditRequest;
use crate::ImageGenerationRequest;
use crate::ImageQuality;
use crate::ImageResponse;
use crate::ImagesClient;
use crate::Provider;
use crate::RequestTelemetry;
use crate::RetryConfig;
use crate::TransportError;
use codex_client::ReqwestTransport;
use codex_http_client::HttpClientBuilder;
use codex_protocol::models::ImageReference;
use http::HeaderMap;
use http::HeaderValue;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;

struct FixtureAuth;

impl AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) {
        headers.insert("authorization", HeaderValue::from_static("Bearer fixture"));
    }
}

#[derive(Default)]
struct RequestCount(AtomicUsize);

impl RequestTelemetry for RequestCount {
    fn on_request(
        &self,
        _attempt: u64,
        _status: Option<StatusCode>,
        _error: Option<&TransportError>,
        _duration: Duration,
    ) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn client(server: &MockServer, name: &str) -> ImagesClient<ReqwestTransport> {
    ImagesClient::new(
        ReqwestTransport::from_http_client(
            HttpClientBuilder::new()
                .build_direct()
                .expect("build fixture HTTP client"),
        ),
        Provider {
            name: name.to_string(),
            base_url: server.uri(),
            query_params: None,
            headers: HeaderMap::from_iter([(
                http::header::HeaderName::from_static("x-provider"),
                HeaderValue::from_static("fixture"),
            )]),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(1),
        },
        Arc::new(FixtureAuth),
    )
}

fn generation() -> ImageGenerationRequest {
    ImageGenerationRequest {
        prompt: "a fox".to_string(),
        background: None,
        model: GROK_IMAGE_MODEL.to_string(),
        n: None,
        quality: None,
        size: None,
    }
}

fn edit(options: &ImageGenerationRequest, images: Vec<ImageReference>) -> ImageEditRequest {
    ImageEditRequest {
        images,
        prompt: options.prompt.clone(),
        background: options.background,
        model: options.model.clone(),
        n: options.n,
        quality: options.quality,
        size: options.size.clone(),
    }
}

fn inline(index: usize) -> ImageReference {
    ImageReference::Inline {
        image_url: format!("data:image/png;base64,fixture{index}"),
    }
}

fn success() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("x-codex-imagegen-request-id", "image-request")
        .insert_header("x-request-id", "outer-request")
        .set_body_json(json!({"created": 42, "data": [{"b64_json": "fixture"}]}))
}

#[tokio::test]
async fn generation_uses_explicit_dialect_and_preserves_telemetry_and_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(success())
        .expect(4)
        .mount(&server)
        .await;
    let options = ImageGenerationRequest {
        background: Some(ImageBackground::Auto),
        n: Some(1),
        quality: Some(ImageQuality::Auto),
        size: Some("auto".to_string()),
        ..generation()
    };
    let mut expected_bodies = Vec::new();
    for name in ["grok", "openai"] {
        for dialect in [ApiDialect::OpenAi, ApiDialect::Grok] {
            let client = client(&server, name);
            let client = match dialect {
                ApiDialect::OpenAi => client,
                ApiDialect::Grok => client.with_dialect(dialect),
            };
            let telemetry = Arc::new(RequestCount::default());
            let client = client.with_telemetry(Some(telemetry.clone()));
            let (response, request_id) = client
                .generate(
                    &options,
                    HeaderMap::from_iter([(
                        http::header::HeaderName::from_static("x-extra"),
                        HeaderValue::from_static("turn"),
                    )]),
                )
                .await
                .expect("generation succeeds");
            assert_eq!(
                (response, request_id),
                (
                    ImageResponse {
                        created: 42,
                        data: vec![ImageData {
                            b64_json: "fixture".to_string(),
                            generation_id: None,
                        }],
                        background: None,
                        quality: None,
                        size: None,
                    },
                    Some("image-request".to_string()),
                )
            );
            assert_eq!(telemetry.0.load(Ordering::SeqCst), 1);
            expected_bodies.push(match dialect {
                ApiDialect::OpenAi => json!({
                    "model": "grok-imagine-image-2.0", "prompt": "a fox", "n": 1,
                    "background": "auto", "quality": "auto", "size": "auto"
                }),
                ApiDialect::Grok => json!({
                    "model": "grok-imagine-image-2.0", "prompt": "a fox",
                    "response_format": "b64_json"
                }),
            });
        }
    }
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), expected_bodies.len());
    for (request, expected) in requests.iter().zip(expected_bodies) {
        assert_eq!(request.url.path(), "/images/generations");
        assert_eq!(
            request.body_json::<Value>().expect("request JSON"),
            expected
        );
        assert_eq!(request.headers["authorization"], "Bearer fixture");
        assert_eq!(request.headers["x-provider"], "fixture");
        assert_eq!(request.headers["x-extra"], "turn");
    }
}

#[tokio::test]
async fn edits_preserve_one_two_three_inline_images_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(success())
        .expect(3)
        .mount(&server)
        .await;
    let client = client(&server, "ordinary-alias").with_dialect(ApiDialect::Grok);
    let mut expected_bodies = Vec::new();
    for count in 1..=3 {
        client
            .edit(
                &edit(&generation(), (0..count).map(inline).collect()),
                HeaderMap::new(),
            )
            .await
            .expect("supported edit succeeds");
        let images: Vec<_> = (0..count)
            .map(|index| json!({"type":"image_url", "url":format!("data:image/png;base64,fixture{index}")}))
            .collect();
        let mut expected = json!({
            "model":"grok-imagine-image-2.0", "prompt":"a fox", "response_format":"b64_json"
        });
        if count == 1 {
            expected["image"] = images[0].clone();
        } else {
            expected["images"] = json!(images);
        }
        expected_bodies.push(expected);
    }
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), expected_bodies.len());
    for (request, expected) in requests.iter().zip(expected_bodies) {
        assert_eq!(request.url.path(), "/images/edits");
        assert_eq!(
            request.body_json::<Value>().expect("request JSON"),
            expected
        );
    }
}

#[tokio::test]
async fn unsupported_options_and_references_fail_before_http() {
    let server = MockServer::start().await;
    let client = client(&server, "ordinary-alias").with_dialect(ApiDialect::Grok);
    let invalid = [
        ImageGenerationRequest {
            model: "other-model".to_string(),
            ..generation()
        },
        ImageGenerationRequest {
            background: Some(ImageBackground::Transparent),
            ..generation()
        },
        ImageGenerationRequest {
            background: Some(ImageBackground::Opaque),
            ..generation()
        },
        ImageGenerationRequest {
            n: Some(0),
            ..generation()
        },
        ImageGenerationRequest {
            n: Some(2),
            ..generation()
        },
        ImageGenerationRequest {
            quality: Some(ImageQuality::Low),
            ..generation()
        },
        ImageGenerationRequest {
            quality: Some(ImageQuality::Medium),
            ..generation()
        },
        ImageGenerationRequest {
            quality: Some(ImageQuality::High),
            ..generation()
        },
        ImageGenerationRequest {
            size: Some("1024x1024".to_string()),
            ..generation()
        },
    ];
    for options in invalid {
        for result in [
            client.generate(&options, HeaderMap::new()).await,
            client
                .edit(&edit(&options, vec![inline(0)]), HeaderMap::new())
                .await,
        ] {
            let (error, request_id) = result.expect_err("unsupported options fail").into_parts();
            assert!(
                matches!(error, ApiError::Stream(message) if message.starts_with("unsupported Grok image request option:"))
            );
            assert_eq!(request_id, None);
        }
    }
    let file = ImageReference::File {
        file_id: "private-file-id".to_string(),
    };
    for images in [
        vec![],
        (0..4).map(inline).collect(),
        (0..5).map(inline).collect(),
        vec![file.clone()],
        vec![inline(0), file.clone()],
        vec![file, inline(0)],
    ] {
        let (error, request_id) = client
            .edit(&edit(&generation(), images), HeaderMap::new())
            .await
            .expect_err("unsupported references fail")
            .into_parts();
        assert!(
            matches!(error, ApiError::Stream(ref message) if message.starts_with("Grok image edits"))
        );
        assert!(!error.to_string().contains("private-file-id"));
        assert_eq!(request_id, None);
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("recorded requests")
            .is_empty()
    );
}

#[tokio::test]
async fn stock_generation_and_file_edit_keep_all_options_and_transparency() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(success().set_body_json(json!({
            "created": 42, "background": "transparent", "quality": "high", "size": "1024x1536",
            "data": [{"b64_json":"fixture", "generation_id":"generation"}]
        })))
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server, "grok");
    let options = ImageGenerationRequest {
        background: Some(ImageBackground::Transparent),
        model: "gpt-image-2".to_string(),
        n: Some(2),
        quality: Some(ImageQuality::High),
        size: Some("1024x1536".to_string()),
        ..generation()
    };
    let edit = edit(
        &options,
        vec![
            inline(0),
            ImageReference::File {
                file_id: "file-image".to_string(),
            },
        ],
    );
    for result in [
        client.generate(&options, HeaderMap::new()).await,
        client.edit(&edit, HeaderMap::new()).await,
    ] {
        assert_eq!(
            result.expect("stock request succeeds").0,
            ImageResponse {
                created: 42,
                data: vec![ImageData {
                    b64_json: "fixture".to_string(),
                    generation_id: Some("generation".to_string())
                }],
                background: Some(ImageBackground::Transparent),
                quality: Some(ImageQuality::High),
                size: Some("1024x1536".to_string()),
            }
        );
    }
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 2);
    let mut expected = json!({
        "model": "gpt-image-2", "prompt": "a fox", "n": 2,
        "background": "transparent", "quality": "high", "size": "1024x1536"
    });
    assert_eq!(
        requests[0].body_json::<Value>().expect("generation body"),
        expected
    );
    expected["images"] = json!([
        {"image_url": "data:image/png;base64,fixture0"}, {"file_id": "file-image"}
    ]);
    assert_eq!(
        requests[1].body_json::<Value>().expect("edit body"),
        expected
    );
}

#[tokio::test]
async fn grok_normalizes_only_missing_created_and_preserves_generation_ids() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(success().set_body_json(json!({
            "data":[{"b64_json":"first","generation_id":"first-id","mime_type":"image/jpeg"},
                    {"b64_json":"second","generation_id":"second-id"},{"b64_json":"legacy"}]
        })))
        .expect(3)
        .mount(&server)
        .await;
    let request = generation();
    let grok_client = client(&server, "alias").with_dialect(ApiDialect::Grok);
    for result in [
        grok_client.generate(&request, HeaderMap::new()).await,
        grok_client
            .edit(&edit(&request, vec![inline(0)]), HeaderMap::new())
            .await,
    ] {
        assert_eq!(
            result.expect("missing Grok timestamp normalizes").0,
            ImageResponse {
                created: 0,
                data: vec![
                    ImageData {
                        b64_json: "first".to_string(),
                        generation_id: Some("first-id".to_string())
                    },
                    ImageData {
                        b64_json: "second".to_string(),
                        generation_id: Some("second-id".to_string())
                    },
                    ImageData {
                        b64_json: "legacy".to_string(),
                        generation_id: None
                    },
                ],
                background: None,
                quality: None,
                size: None,
            }
        );
    }
    let (error, id) = client(&server, "grok")
        .generate(&request, HeaderMap::new())
        .await
        .expect_err("stock still requires timestamp")
        .into_parts();
    assert!(
        matches!(error, ApiError::Stream(message) if message.contains("missing field `created`"))
    );
    assert_eq!(id.as_deref(), Some("image-request"));
}

#[tokio::test]
async fn invalid_responses_and_http_errors_keep_image_request_id_for_both_dialects() {
    let server = MockServer::start().await;
    for dialect in [ApiDialect::OpenAi, ApiDialect::Grok] {
        let client = client(&server, "alias").with_dialect(dialect);
        for body in [
            json!({"created":null,"data":[]}).to_string(),
            json!({"created":"bad","data":[]}).to_string(),
            json!({"created":-1,"data":[]}).to_string(),
            json!({"created":1}).to_string(),
            json!({"created":1,"data":null}).to_string(),
            json!({"created":1,"data":[{}]}).to_string(),
            json!({"created":1,"data":[{"b64_json":1}]}).to_string(),
            json!({"created":1,"data":[{"b64_json":"x","generation_id":1}]}).to_string(),
            json!({"created":1,"data":[],"background":"invalid"}).to_string(),
            json!({"created":1,"data":[],"quality":"invalid"}).to_string(),
            json!({"created":1,"data":[],"size":1}).to_string(),
            "{".to_string(),
            "[]".to_string(),
        ] {
            let mock = Mock::given(method("POST"))
                .respond_with(success().set_body_string(body))
                .expect(2)
                .mount_as_scoped(&server)
                .await;
            for result in [
                client.generate(&generation(), HeaderMap::new()).await,
                client
                    .edit(&edit(&generation(), vec![inline(0)]), HeaderMap::new())
                    .await,
            ] {
                let (error, id) = result.expect_err("invalid response fails").into_parts();
                assert!(
                    matches!(error, ApiError::Stream(message) if message.starts_with("failed to decode image"))
                );
                assert_eq!(id.as_deref(), Some("image-request"));
            }
            drop(mock);
        }
        let mock = Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400)
                    .insert_header("x-codex-imagegen-request-id", "failed-image-request")
                    .insert_header("x-request-id", "outer-request")
                    .set_body_json(json!({"error":{"message":"fixture failure"}})),
            )
            .expect(2)
            .mount_as_scoped(&server)
            .await;
        for result in [
            client.generate(&generation(), HeaderMap::new()).await,
            client
                .edit(&edit(&generation(), vec![inline(0)]), HeaderMap::new())
                .await,
        ] {
            let (error, id) = result
                .expect_err("HTTP failure remains failure")
                .into_parts();
            assert!(matches!(
                error,
                ApiError::Transport(TransportError::Http {
                    status: StatusCode::BAD_REQUEST,
                    ..
                })
            ));
            assert_eq!(id.as_deref(), Some("failed-image-request"));
        }
        drop(mock);
    }
}
