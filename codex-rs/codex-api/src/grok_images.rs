use crate::error::ApiError;
use crate::images::GROK_IMAGE_MODEL;
use crate::images::GROK_MAX_EDIT_IMAGES;
use crate::images::ImageBackground;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageQuality;
use crate::images::ImageResponse;
use codex_protocol::models::ImageReference;
use serde_json::Value;
use serde_json::json;

pub(crate) fn generation_body(request: &ImageGenerationRequest) -> Result<Value, ApiError> {
    validate_options(
        &request.model,
        request.background,
        request.n,
        request.quality,
        request.size.as_deref(),
    )?;
    Ok(image_body(&request.prompt))
}

pub(crate) fn edit_body(request: &ImageEditRequest) -> Result<Value, ApiError> {
    validate_options(
        &request.model,
        request.background,
        request.n,
        request.quality,
        request.size.as_deref(),
    )?;
    if !(1..=GROK_MAX_EDIT_IMAGES).contains(&request.images.len()) {
        return Err(ApiError::Stream(format!(
            "Grok image edits require between 1 and {GROK_MAX_EDIT_IMAGES} images"
        )));
    }
    let images = request
        .images
        .iter()
        .map(|image| match image {
            ImageReference::Inline { image_url } => Ok(json!({
                "type": "image_url",
                "url": image_url,
            })),
            ImageReference::File { .. } => Err(ApiError::Stream(
                "Grok image edits do not support file-backed image references".to_string(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = image_body(&request.prompt);
    if let [image] = images.as_slice() {
        body["image"] = image.clone();
    } else {
        body["images"] = Value::Array(images);
    }
    Ok(body)
}

fn image_body(prompt: &str) -> Value {
    json!({"model": GROK_IMAGE_MODEL, "prompt": prompt, "response_format": "b64_json"})
}

// Only the retained single-image defaults are supported. Reject explicit
// unsupported options rather than silently dropping the caller's intent.
fn validate_options(
    model: &str,
    background: Option<ImageBackground>,
    n: Option<u64>,
    quality: Option<ImageQuality>,
    size: Option<&str>,
) -> Result<(), ApiError> {
    let unsupported = if model != GROK_IMAGE_MODEL {
        Some("model")
    } else if !matches!(background, None | Some(ImageBackground::Auto)) {
        Some("background")
    } else if !matches!(n, None | Some(1)) {
        Some("n")
    } else if !matches!(quality, None | Some(ImageQuality::Auto)) {
        Some("quality")
    } else if !matches!(size, None | Some("auto")) {
        Some("size")
    } else {
        None
    };
    match unsupported {
        Some(field) => Err(ApiError::Stream(format!(
            "unsupported Grok image request option: {field}"
        ))),
        None => Ok(()),
    }
}

// Missing timestamps are the sole response relaxation; malformed present
// fields continue through the ordinary typed response decoder.
pub(crate) fn decode_response(body: &[u8]) -> Result<ImageResponse, serde_json::Error> {
    let mut value: Value = serde_json::from_slice(body)?;
    if let Some(object) = value.as_object_mut() {
        object.entry("created").or_insert_with(|| json!(0));
    }
    serde_json::from_value(value)
}

#[cfg(test)]
#[path = "grok_images_tests.rs"]
mod tests;
