use crate::error::ApiError;
use crate::images::GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES;
use crate::images::ImageEditRequest;
use crate::images::ImageGenerationRequest;
use crate::images::ImageResponse;
use codex_protocol::models::ImageReference;
use serde_json::Value;

const GROK_IMAGE_MODEL: &str = "grok-imagine-image-2.0";

/// Projects a stock image-generation request onto the verified Grok wire shape.
///
/// Background/quality/count/size are intentionally not invented on the Grok
/// wire; stock/OpenAI keeps those fields unchanged on its own dialect.
pub(crate) fn generation_body(request: &ImageGenerationRequest) -> Value {
    serde_json::json!({
        "model": GROK_IMAGE_MODEL,
        "prompt": request.prompt.as_str(),
        "response_format": "b64_json",
    })
}

/// Projects a stock image-edit request onto the verified Grok wire shape.
pub(crate) fn edit_body(request: &ImageEditRequest) -> Result<Value, ApiError> {
    if !(1..=GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES).contains(&request.images.len()) {
        return Err(ApiError::Stream(format!(
            "Grok image edits require between 1 and {GROK_IMAGE_GENERATION_MAX_EDIT_IMAGES} images"
        )));
    }

    let mut images = Vec::with_capacity(request.images.len());
    for image in &request.images {
        let image_url = match image {
            ImageReference::Inline { image_url } => image_url,
            ImageReference::File { file_id } => {
                return Err(ApiError::Stream(format!(
                    "Grok image edits do not support file-backed image reference `{file_id}`"
                )));
            }
        };
        images.push(serde_json::json!({
            "type": "image_url",
            "url": image_url,
        }));
    }

    let image_field = if images.len() == 1 { "image" } else { "images" };
    let image_value = if images.len() == 1 {
        images[0].clone()
    } else {
        Value::Array(images)
    };
    let mut body = serde_json::Map::new();
    body.insert(
        "model".to_string(),
        Value::String(GROK_IMAGE_MODEL.to_string()),
    );
    body.insert("prompt".to_string(), Value::String(request.prompt.clone()));
    body.insert(
        "response_format".to_string(),
        Value::String("b64_json".to_string()),
    );
    body.insert(image_field.to_string(), image_value);
    Ok(Value::Object(body))
}

/// Decodes a Grok image response, normalizing the backend's optional timestamp.
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
