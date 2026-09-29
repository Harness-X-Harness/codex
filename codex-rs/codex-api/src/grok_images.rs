use crate::error::ApiError;
use crate::images::GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageResponse;
use serde_json::Value;

const GROK_IMAGE_MODEL: &str = "grok-imagine-image-2.0";

/// Projects a stock image-generation request onto the Grok images contract.
pub(crate) fn generation_body(request: &ImageGenerationRequest) -> Value {
    serde_json::json!({
        "model": GROK_IMAGE_MODEL,
        "prompt": request.prompt.as_str(),
        "response_format": "b64_json",
    })
}

/// Projects a stock image-edit request onto the Grok images contract.
///
/// Cardinality is fail-closed: Grok accepts 1..=3 images and uses `image`
/// for a single reference or `images` for multiple.
pub(crate) fn edit_body(request: &ImageEditRequest) -> Result<Value, ApiError> {
    if !(1..=GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES).contains(&request.images.len()) {
        return Err(ApiError::Stream(format!(
            "Grok image edits require between 1 and {GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES} images"
        )));
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
        "model": GROK_IMAGE_MODEL,
        "prompt": request.prompt.as_str(),
        "response_format": "b64_json",
    });
    if let Some(object) = body.as_object_mut() {
        object.insert(
            image_field.to_string(),
            if images.len() == 1 {
                images[0].clone()
            } else {
                Value::Array(images)
            },
        );
    }
    Ok(body)
}

/// Decodes a Grok image response, defaulting a missing `created` timestamp.
pub(crate) fn decode_response(body: &[u8], operation: &str) -> Result<ImageResponse, ApiError> {
    let mut value: Value = serde_json::from_slice(body).map_err(|error| {
        ApiError::Stream(format!("failed to decode {operation} response: {error}"))
    })?;
    if let Some(object) = value.as_object_mut() {
        object
            .entry("created".to_string())
            .or_insert_with(|| serde_json::json!(0));
    }
    serde_json::from_value(value).map_err(|error| {
        ApiError::Stream(format!("failed to decode {operation} response: {error}"))
    })
}

#[cfg(test)]
#[path = "grok_images_tests.rs"]
mod tests;
