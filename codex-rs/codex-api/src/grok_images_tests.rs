use pretty_assertions::assert_eq;
use serde_json::json;

use super::GROK_IMAGE_MODEL;
use super::decode_response;
use super::edit_body;
use super::generation_body;
use crate::images::GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES;
use crate::images::ImageBackground;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageQuality;
use crate::images::ImageUrl;

fn generation_request() -> ImageGenerationRequest {
    ImageGenerationRequest {
        prompt: "draw a fox".to_string(),
        background: Some(ImageBackground::Transparent),
        model: "gpt-image-2".to_string(),
        n: Some(2),
        quality: Some(ImageQuality::High),
        size: Some("1024x1024".to_string()),
    }
}

#[test]
fn generation_projects_grok_wire_and_drops_openai_only_fields() {
    assert_eq!(
        generation_body(&generation_request()),
        json!({
            "model": GROK_IMAGE_MODEL,
            "prompt": "draw a fox",
            "response_format": "b64_json",
        })
    );
}

#[test]
fn edit_projects_single_image_field() {
    let body = edit_body(&ImageEditRequest {
        images: vec![ImageUrl {
            image_url: "one".to_string(),
        }],
        prompt: "edit".to_string(),
        background: None,
        model: "gpt-image-2".to_string(),
        n: None,
        quality: None,
        size: None,
    })
    .expect("single image should encode");
    assert_eq!(
        body,
        json!({
            "model": GROK_IMAGE_MODEL,
            "prompt": "edit",
            "response_format": "b64_json",
            "image": {"type": "image_url", "url": "one"},
        })
    );
}

#[test]
fn edit_projects_multiple_images_field() {
    let body = edit_body(&ImageEditRequest {
        images: vec![
            ImageUrl {
                image_url: "one".to_string(),
            },
            ImageUrl {
                image_url: "two".to_string(),
            },
        ],
        prompt: "edit".to_string(),
        background: None,
        model: "gpt-image-2".to_string(),
        n: None,
        quality: None,
        size: None,
    })
    .expect("two images should encode");
    assert_eq!(
        body,
        json!({
            "model": GROK_IMAGE_MODEL,
            "prompt": "edit",
            "response_format": "b64_json",
            "images": [
                {"type": "image_url", "url": "one"},
                {"type": "image_url", "url": "two"}
            ],
        })
    );
}

#[test]
fn edit_rejects_unsupported_cardinality() {
    for count in [0, GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES + 1] {
        edit_body(&ImageEditRequest {
            images: (0..count)
                .map(|index| ImageUrl {
                    image_url: format!("image-{index}"),
                })
                .collect(),
            prompt: "edit".to_string(),
            background: None,
            model: "gpt-image-2".to_string(),
            n: None,
            quality: None,
            size: None,
        })
        .expect_err("unsupported image count should fail");
    }
}

#[test]
fn decode_defaults_missing_created() {
    let response = decode_response(
        br#"{"data":[{"b64_json":"REDACT","mime_type":"image/jpeg"}]}"#,
        "image generation",
    )
    .expect("Grok response should decode");
    assert_eq!(response.created, 0);
    assert_eq!(response.data[0].b64_json, "REDACT");
}
