use super::GROK_IMAGE_MODEL;
use super::decode_response;
use super::edit_body;
use super::generation_body;
use crate::images::GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES;
use crate::images::ImageBackground;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageQuality;
use codex_protocol::models::ImageReference;
use pretty_assertions::assert_eq;
use serde_json::json;

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
fn generation_projects_only_verified_grok_fields() {
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
fn edit_projects_inline_images_and_enforces_grok_cardinality() {
    let request = ImageEditRequest {
        images: vec![
            ImageReference::Inline {
                image_url: "one".to_string(),
            },
            ImageReference::Inline {
                image_url: "two".to_string(),
            },
        ],
        prompt: "edit".to_string(),
        background: Some(ImageBackground::Transparent),
        model: "gpt-image-2".to_string(),
        n: None,
        quality: None,
        size: None,
    };
    assert_eq!(
        edit_body(&request).expect("inline Grok edit should encode"),
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

    let mut too_many = request;
    too_many.images = (0..=GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES)
        .map(|index| ImageReference::Inline {
            image_url: format!("image-{index}"),
        })
        .collect();
    edit_body(&too_many).expect_err("Grok edit cardinality must fail closed");
}

#[test]
fn edit_rejects_new_file_backed_reference_until_backend_support_is_verified() {
    let request = ImageEditRequest {
        images: vec![ImageReference::File {
            file_id: "file-image".to_string(),
        }],
        prompt: "edit".to_string(),
        background: None,
        model: "gpt-image-2".to_string(),
        n: None,
        quality: None,
        size: None,
    };
    let error = edit_body(&request).expect_err("file-backed Grok edit must fail closed");
    assert!(
        error.to_string().contains("file-backed image reference"),
        "{error}"
    );
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
