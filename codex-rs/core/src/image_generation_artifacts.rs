use codex_protocol::items::TurnItem;
use codex_utils_image::materialize_image_generation_artifact;

use crate::session::session::Session;

/// Adds the derived local artifact path required by App Server image clients.
/// The provider result remains the durable, lossless response item.
pub(crate) async fn materialize_image_generation_turn_item(
    session: &Session,
    turn_item: &mut TurnItem,
) {
    let TurnItem::ImageGeneration(image) = turn_item else {
        return;
    };
    if image.status != "completed" || image.result.is_empty() || image.saved_path.is_some() {
        return;
    }

    let codex_home = session.codex_home().await;
    match materialize_image_generation_artifact(
        &codex_home,
        &session.thread_id().to_string(),
        &image.id,
        &image.result,
    )
    .await
    {
        Ok(path) => image.saved_path = Some(path),
        Err(error) => tracing::warn!(
            item_id = %image.id,
            "failed to materialize generated image for App Server: {error}"
        ),
    }
}
