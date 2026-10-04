use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolOutput;
use codex_extension_items::ExtensionItem;
use codex_extension_items::image_generation::ImageGenerationItem;
use codex_model_provider_info::WireApi;
use codex_protocol::models::ContentItem;
use codex_protocol::models::DEFAULT_IMAGE_DETAIL;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ImageGenerationBeginEvent;
use codex_protocol::protocol::ImageGenerationEndEvent;
use codex_utils_image::PromptImageMode;
use codex_utils_image::load_for_prompt_bytes;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[path = "grok_composition_support.rs"]
mod support;
use support::Harness;
use support::HttpFixture;
use support::Items;
use support::call;
use support::provider;

// Actual 2x1 RGBA PNGs (including a translucent pixel) and RGB JPEG. Inline
// fixtures avoid adding filesystem/build-data dependencies to these tests.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR4nGP4z8DQwMDw/z8ADX4DfuoeFyEAAAAASUVORK5CYII=";
const EDITED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR4nGNk+M/wn4GBgQEADAYCAIT7C4AAAAAASUVORK5CYII=";
const JPEG: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAIDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDyK7/4/J/+ujfzooor9nwX+7U/8K/I5cX/ALxU9X+Z/9k=";

// Fully decodable 1-bit PNG; its RGBA pixel budget exceeds the local 32 MiB cap.
const OVERSIZED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAIAAAAAQBAQAAAACqfzneAAAEEUlEQVR42u3BMQEAAADCoPVPbQZ/oAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABOAwjxAAF+Hn7tAAAAAElFTkSuQmCC";

fn image_response(result: &str, claimed_mime: &str) -> Value {
    json!({"data":[{"b64_json":result,"mime_type":claimed_mime,"generation_id":"composition-generation"}]})
}

fn history_image(image: ImageReference) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputImage {
            image,
            detail: None,
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn terminal(items: &Items, id: &str, status: &str) -> ImageGenerationItem {
    let events = std::mem::take(&mut *items.0.lock().expect("emitted items"));
    let [(start_phase, started), (end_phase, completed)] = events.as_slice() else {
        panic!("expected exactly started and completed lifecycle events");
    };
    let ExtensionItem::ImageGeneration(started_image) = &started.item else {
        panic!("expected started image item");
    };
    let ExtensionItem::ImageGeneration(completed_image) = &completed.item else {
        panic!("expected completed image item");
    };
    assert_eq!((*start_phase, *end_phase), ("started", "completed"));
    assert_eq!(
        started_image,
        &ImageGenerationItem {
            id: id.to_string(),
            status: "in_progress".to_string(),
            revised_prompt: None,
            result: String::new(),
            transparent_background: None,
            failure: None,
            saved_path: None,
            imagegen_request_id: None,
            generation_id: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&started.legacy_events).expect("legacy start"),
        json!([EventMsg::ImageGenerationBegin(ImageGenerationBeginEvent {
            call_id: id.to_string()
        })])
    );
    assert_eq!(
        (completed_image.id.as_str(), completed_image.status.as_str()),
        (id, status)
    );
    assert_eq!(
        serde_json::to_value(&completed.legacy_events).expect("legacy completion"),
        json!([EventMsg::ImageGenerationEnd(ImageGenerationEndEvent {
            call_id: id.to_string(),
            status: status.to_string(),
            revised_prompt: completed_image.revised_prompt.clone(),
            result: completed_image.result.clone(),
            transparent_background: completed_image.transparent_background,
            failure: completed_image.failure.clone(),
            saved_path: completed_image.saved_path.clone(),
        })])
    );
    completed_image.clone()
}

fn assert_outputs(
    output: &dyn ToolOutput,
    call: &ToolCall<'_>,
    item: &ImageGenerationItem,
) -> ResponseItem {
    let saved_path = item.saved_path.as_ref().expect("saved image path");
    let encoded = BASE64_STANDARD.decode(&item.result).expect("result base64");
    assert_eq!(
        std::fs::read(saved_path).expect("saved image bytes"),
        encoded
    );
    let decoded = load_for_prompt_bytes(
        Path::new("result.png"),
        encoded.clone(),
        PromptImageMode::Original,
    )
    .expect("fully decoded image");
    assert_eq!(
        (decoded.mime.as_str(), decoded.width, decoded.height),
        ("image/png", 2, 1)
    );
    assert_eq!(decoded.bytes.as_ref(), encoded);
    let code_result = output.code_mode_result(&call.payload);
    let image_url = format!("data:image/png;base64,{}", item.result);
    let hint = code_result["output_hint"]
        .as_str()
        .expect("saved image hint");
    assert!(hint.contains(saved_path.as_path().to_str().expect("path text")));
    assert_eq!(
        code_result,
        json!({"image_url":image_url, "output_hint":hint})
    );
    let response = output.to_response_item(&call.call_id, &call.payload);
    assert_eq!(
        response,
        ResponseInputItem::FunctionCallOutput {
            call_id: call.call_id.clone(),
            output: FunctionCallOutputPayload {
                body: FunctionCallOutputBody::ContentItems(vec![
                    FunctionCallOutputContentItem::InputImage {
                        image: ImageReference::Inline { image_url },
                        detail: Some(DEFAULT_IMAGE_DETAIL)
                    },
                    FunctionCallOutputContentItem::InputText {
                        text: hint.to_string()
                    },
                ]),
                success: Some(true),
            },
        }
    );
    assert_eq!(output.log_output(), "[generated image]");
    assert!(output.success_for_logging());
    let stored = serde_json::to_value(item).expect("persisted item");
    assert!(stored.get("imagegenRequestId").is_none());
    assert!(stored.get("generationId").is_none());
    let mut expected_stored = item.clone();
    expected_stored.imagegen_request_id = None;
    expected_stored.generation_id = None;
    assert_eq!(
        serde_json::from_value::<ImageGenerationItem>(stored).expect("restored item"),
        expected_stored
    );
    response.into()
}

#[tokio::test(flavor = "multi_thread")]
async fn installed_grok_generation_normalizes_saved_output_and_edits_the_same_history_image() {
    for (source, claimed_mime) in [(PNG, "image/jpeg"), (JPEG, "image/png")] {
        let server = HttpFixture::new(vec![
            (200, image_response(source, claimed_mime)),
            (200, image_response(EDITED_PNG, "image/png")),
        ])
        .await;
        let harness = Harness::new(provider(
            &server.url,
            "ordinary-alias",
            WireApi::GrokResponses,
        ))
        .await;
        let tool = harness.tool();
        let items = Arc::new(Items::default());
        let generate = call(
            "generate",
            json!({"prompt":"a fox"}),
            vec![],
            items.clone(),
            ToolCallSource::Direct,
        );
        let output = tool
            .handle(generate.clone())
            .await
            .expect("generation succeeds");
        let generated = terminal(&items, "generate", "completed");
        assert_eq!(
            (
                generated.imagegen_request_id.as_deref(),
                generated.generation_id.as_deref(),
                generated.transparent_background
            ),
            (
                Some("composition-image-request"),
                Some("composition-generation"),
                None
            )
        );
        assert_eq!(generated.revised_prompt.as_deref(), Some("a fox"));
        if source == PNG {
            assert_eq!(
                generated.result, source,
                "PNG passthrough preserves alpha and pixels"
            );
        } else {
            assert_ne!(generated.result, source, "JPEG has become actual PNG");
        }
        let history = assert_outputs(output.as_ref(), &generate, &generated);
        let edit = call(
            "edit",
            json!({"prompt":"make it green", "num_last_images_to_include":1}),
            vec![history],
            items.clone(),
            ToolCallSource::CodeMode {
                cell_id: "cell".to_string(),
                runtime_tool_call_id: "nested-image".to_string(),
            },
        );
        let output = tool
            .handle(edit.clone())
            .await
            .expect("history edit succeeds");
        let edited = terminal(&items, "edit", "completed");
        assert_eq!(edited.result, EDITED_PNG);
        assert_ne!(edited.result, generated.result);
        assert_outputs(output.as_ref(), &edit, &edited);
        let requests = server.requests().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/images/generations");
        assert_eq!(
            requests[0].body,
            json!({"model":"grok-imagine-image-2.0", "prompt":"a fox", "response_format":"b64_json"})
        );
        assert_eq!(requests[1].path, "/images/edits");
        assert_eq!(
            requests[1].body,
            json!({"model":"grok-imagine-image-2.0", "prompt":"make it green", "response_format":"b64_json",
            "image":{"type":"image_url", "url":format!("data:image/png;base64,{}",generated.result)}})
        );
        for (request, turn) in requests.iter().zip(["turn-generate", "turn-edit"]) {
            assert_eq!(request.headers["authorization"], "Bearer fixture-provider");
            assert_eq!(request.headers["originator"], "composition-originator");
            assert_eq!(request.headers["x-codex-image-turn-id"], turn);
        }
    }
}

#[tokio::test]
async fn installed_schema_and_runtime_reject_unsupported_grok_requests_before_http() {
    let server = HttpFixture::new(vec![]).await;
    let harness = Harness::new(provider(&server.url, "OpenAI", WireApi::GrokResponses)).await;
    let tool = harness.tool();
    let schema = serde_json::to_value(tool.spec()).expect("tool spec");
    let function = &schema["tools"][0];
    let properties = &function["parameters"]["properties"];
    assert!(properties.get("transparent_background").is_none());
    assert_eq!(properties["referenced_image_paths"]["maxItems"], json!(3));
    assert_eq!(
        properties["num_last_images_to_include"]["maximum"],
        json!(3)
    );
    assert_eq!(
        properties["num_last_images_to_include"]["minimum"].as_f64(),
        Some(1.0)
    );
    let description = function["description"].as_str().expect("description");
    assert!(description.contains("up to 3"));
    assert!(!description.contains("up to 5"));
    assert!(!description.contains("transparent_background"));
    let items = Arc::new(Items::default());
    let paths = (0..4)
        .map(|index| {
            harness
                .config
                .codex_home
                .join(format!("missing-{index}.png"))
        })
        .collect::<Vec<_>>();
    let mut invalid = vec![
        (
            json!(["x", true, null, null]),
            "arguments must be an object",
        ),
        (
            json!(["x", false, null, null]),
            "arguments must be an object",
        ),
        (Value::Null, "arguments must be an object"),
        (json!("x"), "arguments must be an object"),
        (
            json!({"prompt":"x", "referenced_image_paths":paths}),
            "at most 3",
        ),
        (
            json!({"prompt":"x", "num_last_images_to_include":0}),
            "between 1 and 3",
        ),
        (
            json!({"prompt":"x", "num_last_images_to_include":4}),
            "between 1 and 3",
        ),
        (
            json!({"prompt":"x", "num_last_images_to_include":1}),
            "only 0 were available",
        ),
        (
            json!({"prompt":"x", "referenced_image_paths":[paths[0]], "num_last_images_to_include":1}),
            "provide only one",
        ),
    ];
    for value in [json!(false), json!(true), Value::Null, json!("false")] {
        invalid.push((
            json!({"prompt":"x", "transparent_background":value}),
            "transparent_background is not supported",
        ));
    }
    for (arguments, message) in invalid {
        let error = tool
            .handle(call(
                "invalid",
                arguments,
                vec![],
                items.clone(),
                ToolCallSource::Direct,
            ))
            .await
            .err()
            .expect("invalid input fails");
        assert!(error.to_string().contains(message), "{error}");
        assert!(
            items.0.lock().expect("items").is_empty(),
            "validation precedes lifecycle and filesystem reads"
        );
    }
    let inline = history_image(ImageReference::Inline {
        image_url: format!("data:image/png;base64,{PNG}"),
    });
    let file = history_image(ImageReference::File {
        file_id: "selected-file".to_string(),
    });
    for count in [1, 2] {
        let error = tool
            .handle(call(
                "file",
                json!({"prompt":"x", "num_last_images_to_include":count}),
                vec![inline.clone(), file.clone()],
                items.clone(),
                ToolCallSource::Direct,
            ))
            .await
            .err()
            .expect("selected File remains unsupported");
        assert!(error.to_string().contains("file-backed image references"));
        let failed = terminal(&items, "file", "failed");
        assert_eq!(
            (failed.result, failed.saved_path, failed.imagegen_request_id),
            (String::new(), None, None)
        );
    }
    assert!(server.requests().await.is_empty());
    assert!(!harness.config.codex_home.join("artifacts").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn config_changes_refresh_dialect_availability_auth_route_and_stock_file_transparency() {
    let grok = HttpFixture::new(vec![(200, image_response(PNG, "image/png"))]).await;
    let stock = HttpFixture::new(vec![
        (
            200,
            json!({"created":1,"background":"transparent","data":[{"b64_json":PNG}]}),
        ),
        (
            200,
            json!({"created":1,"background":"opaque","data":[{"b64_json":PNG}]}),
        ),
    ])
    .await;
    let mut harness = Harness::new(provider(&grok.url, "grok", WireApi::Responses)).await;
    assert!(
        harness.tools().is_empty(),
        "a name does not enable Grok images"
    );
    let mut config = harness.config.clone();
    config.model_provider.wire_api = WireApi::GrokResponses;
    harness.refresh(config);
    let items = Arc::new(Items::default());
    harness
        .tool()
        .handle(call(
            "grok",
            json!({"prompt":"x"}),
            vec![],
            items.clone(),
            ToolCallSource::Direct,
        ))
        .await
        .expect("Grok config is active");
    terminal(&items, "grok", "completed");
    let mut config = harness.config.clone();
    config.model_provider = provider(
        &format!("{}/refreshed", stock.url),
        "grok",
        WireApi::Responses,
    );
    config.model_provider.experimental_bearer_token = None;
    config.model_provider.requires_openai_auth = true;
    config.codex_home = config.codex_home.join("refreshed");
    harness.refresh(config);
    let tool = harness.tool();
    let spec = serde_json::to_value(tool.spec()).expect("stock spec");
    let properties = &spec["tools"][0]["parameters"]["properties"];
    assert!(properties.get("transparent_background").is_some());
    assert_eq!(properties["referenced_image_paths"]["maxItems"], json!(5));
    assert_eq!(
        properties["num_last_images_to_include"]["maximum"].as_f64(),
        Some(5.0)
    );
    assert_eq!(
        properties["num_last_images_to_include"]["minimum"].as_f64(),
        Some(1.0)
    );
    for background in [Value::Null, json!("false")] {
        assert!(
            tool.handle(call(
                "invalid-stock",
                json!({"prompt":"x", "transparent_background":background}),
                vec![],
                items.clone(),
                ToolCallSource::Direct
            ))
            .await
            .is_err()
        );
        assert!(items.0.lock().expect("items").is_empty());
    }
    assert!(stock.requests().await.is_empty());
    let history = vec![history_image(ImageReference::File {
        file_id: "stock-file".to_string(),
    })];
    let edit = call(
        "stock-edit",
        json!({"prompt":"x", "transparent_background":true,"num_last_images_to_include":1}),
        history,
        items.clone(),
        ToolCallSource::Direct,
    );
    let output = tool
        .handle(edit.clone())
        .await
        .expect("stock File edit succeeds");
    let item = terminal(&items, "stock-edit", "completed");
    assert_eq!(item.transparent_background, Some(true));
    assert!(
        item.saved_path
            .as_ref()
            .expect("refreshed save path")
            .as_path()
            .starts_with(harness.config.codex_home.as_path())
    );
    assert_outputs(output.as_ref(), &edit, &item);
    tool.handle(call(
        "stock-generate",
        json!({"prompt":"x", "transparent_background":false}),
        vec![],
        items.clone(),
        ToolCallSource::Direct,
    ))
    .await
    .expect("explicit stock false succeeds");
    assert_eq!(
        terminal(&items, "stock-generate", "completed").transparent_background,
        Some(false)
    );
    let requests = stock.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/refreshed/images/edits");
    assert_eq!(
        requests[0].body,
        json!({"model":"gpt-image-2","prompt":"x","background":"transparent","quality":"auto","size":"auto","images":[{"file_id":"stock-file"}]})
    );
    assert_eq!(
        requests[1].body,
        json!({"model":"gpt-image-2","prompt":"x","background":"opaque","quality":"auto","size":"auto"})
    );
    for request in requests {
        assert_eq!(request.headers["authorization"], "Bearer fixture-auth");
    }
    assert_eq!(grok.requests().await.len(), 1);
    assert_eq!(
        grok.requests().await[0].body["model"],
        json!("grok-imagine-image-2.0")
    );
    for (name, actor, available) in [
        ("OpenAI", false, true),
        ("ordinary-alias", true, true),
        ("grok", false, false),
    ] {
        let mut config = harness.config.clone();
        config.model_provider = provider(&grok.url, name, WireApi::Responses);
        if actor {
            config.model_provider.http_headers = Some(
                [(
                    "x-openai-actor-authorization".to_string(),
                    "fixture-actor".into(),
                )]
                .into(),
            );
        }
        harness.refresh(config);
        assert_eq!(!harness.tools().is_empty(), available);
    }
}

#[tokio::test]
async fn backend_and_codec_failures_complete_the_started_item_without_saving_an_artifact() {
    let png = BASE64_STANDARD.decode(PNG).expect("PNG fixture");
    let missing_iend = BASE64_STANDARD.encode(&png[..png.len() - 12]);
    let mut corrupt_iend = png;
    *corrupt_iend.last_mut().expect("IEND checksum") ^= 1;
    let corrupt_iend = BASE64_STANDARD.encode(corrupt_iend);
    for (response, expected_error) in [
        (
            (200, image_response(&missing_iend, "image/png")),
            "unable to normalize generated image",
        ),
        (
            (200, image_response(&corrupt_iend, "image/png")),
            "unable to normalize generated image",
        ),
        (
            (400, json!({"error":{"message":"fixture denial"}})),
            "image generation failed",
        ),
        (
            (200, json!({"created":1})),
            "failed to decode image generation response",
        ),
        ((200, json!({"data":[]})), "returned no image data"),
        (
            (200, image_response("invalid base64", "image/png")),
            "not valid base64",
        ),
        (
            (200, image_response("iVBORw0KGgo=", "image/png")),
            "unable to normalize generated image",
        ),
        (
            (200, image_response(OVERSIZED_PNG, "image/png")),
            "decoded pixels is too large",
        ),
        (
            (
                200,
                image_response(
                    "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7",
                    "image/gif",
                ),
            ),
            "unsupported image",
        ),
    ] {
        let server = HttpFixture::new(vec![response]).await;
        let harness = Harness::new(provider(&server.url, "alias", WireApi::GrokResponses)).await;
        let items = Arc::new(Items::default());
        let error = harness
            .tool()
            .handle(call(
                "failed",
                json!({"prompt":"x"}),
                vec![],
                items.clone(),
                ToolCallSource::Direct,
            ))
            .await
            .err()
            .expect("failure must not return an image");
        assert!(error.to_string().contains(expected_error), "{error}");
        let failed = terminal(&items, "failed", "failed");
        assert_eq!(
            (
                failed.result,
                failed.saved_path,
                failed.generation_id,
                failed.imagegen_request_id
            ),
            (
                String::new(),
                None,
                None,
                Some("composition-image-request".to_string())
            )
        );
        assert_eq!(server.requests().await.len(), 1);
        assert!(!harness.config.codex_home.join("artifacts").exists());
    }
}
