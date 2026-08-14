use std::fmt::Display;

pub(crate) use codex_utils_image::image_generation_artifact_path;
pub(crate) use codex_utils_image::materialize_image_generation_artifact;

const MAX_IMAGE_GENERATION_OUTPUT_HINT_BYTES: usize = 1024;

/// Returns the model-facing generated-image path hint, or omits it if it is too large.
pub(crate) fn image_generation_output_hint(
    image_output_dir: impl Display,
    image_output_path: impl Display,
) -> Option<String> {
    let hint = format!(
        "Generated images are saved to {image_output_dir} as {image_output_path} by default.\nIf you need to use a generated image at another path, copy it and leave the original in place unless the user explicitly asks you to delete it.\nThe generated image is already displayed to the user. There is no need to render it in the final response as a Markdown image or file link."
    );
    (hint.len() <= MAX_IMAGE_GENERATION_OUTPUT_HINT_BYTES).then_some(hint)
}
