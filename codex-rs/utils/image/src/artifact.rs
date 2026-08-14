use std::io;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use codex_utils_absolute_path::AbsolutePathBuf;

const GENERATED_IMAGE_ARTIFACTS_DIR: &str = "generated_images";

/// Returns the shared artifact path for generated images.
pub fn image_generation_artifact_path(
    save_root: &AbsolutePathBuf,
    session_id: &str,
    call_id: &str,
) -> AbsolutePathBuf {
    let sanitize = |value: &str| {
        let mut sanitized: String = value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        if sanitized.is_empty() {
            sanitized = "generated_image".to_string();
        }
        sanitized
    };

    save_root
        .join(GENERATED_IMAGE_ARTIFACTS_DIR)
        .join(sanitize(session_id))
        .join(format!("{}.png", sanitize(call_id)))
}

/// Materializes a generated-image result without changing its durable response item.
///
/// OpenAI image responses contain raw base64. Grok Responses contain a PNG data
/// URL. Both forms produce the same local artifact contract for App clients.
pub async fn materialize_image_generation_artifact(
    save_root: &AbsolutePathBuf,
    session_id: &str,
    call_id: &str,
    result: &str,
) -> io::Result<AbsolutePathBuf> {
    let bytes = decode_image_generation_result(result)?;
    let path = image_generation_artifact_path(save_root, session_id, call_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent.as_path()).await?;
    }
    tokio::fs::write(path.as_path(), bytes).await?;
    Ok(path)
}

fn decode_image_generation_result(result: &str) -> io::Result<Vec<u8>> {
    let result = result.trim();
    let payload = if let Some((metadata, payload)) = result.split_once(',') {
        if !metadata.eq_ignore_ascii_case("data:image/png;base64") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported image generation data URL",
            ));
        }
        payload.trim()
    } else {
        result
    };
    BASE64_STANDARD
        .decode(payload.as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::decode_image_generation_result;

    #[test]
    fn decodes_raw_and_png_data_url_results_without_changing_bytes() {
        let raw = "AQIDBA==";
        assert_eq!(decode_image_generation_result(raw).unwrap(), [1, 2, 3, 4]);
        assert_eq!(
            decode_image_generation_result(&format!("data:image/png;base64,{raw}")).unwrap(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn rejects_unproven_image_data_url_types() {
        assert!(decode_image_generation_result("data:image/jpeg;base64,AQIDBA==").is_err());
    }
}
