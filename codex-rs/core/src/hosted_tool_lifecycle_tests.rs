use codex_protocol::items::ImageGenerationItem;
use codex_protocol::items::TurnItem;

use super::HostedToolCompletion;
use super::HostedToolLifecycle;
use crate::tools::grok_hosted_output::GrokHostedOutputOwner;

fn image_item(id: &str, status: &str) -> TurnItem {
    TurnItem::ImageGeneration(ImageGenerationItem {
        id: id.to_string(),
        status: status.to_string(),
        revised_prompt: None,
        prompt: Some("Draw a fox.".to_string()),
        result: String::new(),
        saved_path: None,
    })
}

#[test]
fn grok_hosted_lifecycle_emits_only_one_start_and_one_terminal() {
    let mut lifecycle = HostedToolLifecycle::default();
    let item = image_item("ui-image-1", "in_progress");

    assert_eq!(
        lifecycle.record_added(
            "provider-image-1",
            GrokHostedOutputOwner::ImageGeneration,
            "ig",
        ),
        Ok(true)
    );
    lifecycle.attach_started_item("provider-image-1", item.clone(), true);
    assert_eq!(
        lifecycle.record_added(
            "provider-image-1",
            GrokHostedOutputOwner::ImageGeneration,
            "ig",
        ),
        Ok(false)
    );
    let HostedToolCompletion::First {
        started_item: Some(started),
    } = lifecycle
        .record_completed(
            "provider-image-1",
            GrokHostedOutputOwner::ImageGeneration,
            "ig",
        )
        .expect("matching owner should complete")
    else {
        panic!("expected the first hosted completion");
    };
    assert_eq!(started.id(), item.id());
    assert!(matches!(
        lifecycle.record_completed(
            "provider-image-1",
            GrokHostedOutputOwner::ImageGeneration,
            "ig",
        ),
        Ok(HostedToolCompletion::Duplicate)
    ));
}

#[test]
fn grok_hosted_lifecycle_reports_a_completion_without_an_observed_start() {
    let mut lifecycle = HostedToolLifecycle::default();

    assert!(matches!(
        lifecycle.record_completed("web-1", GrokHostedOutputOwner::WebSearch, "ws"),
        Ok(HostedToolCompletion::First { started_item: None })
    ));
}

#[test]
fn grok_incomplete_hosted_items_fail_in_memory_without_creating_a_durable_item() {
    let mut lifecycle = HostedToolLifecycle::default();
    assert_eq!(
        lifecycle.record_added("image-1", GrokHostedOutputOwner::ImageGeneration, "ig"),
        Ok(true)
    );
    lifecycle.attach_started_item("image-1", image_item("image-1", "in_progress"), true);

    let incomplete = lifecycle.take_incomplete();
    assert_eq!(incomplete.len(), 1);
    let TurnItem::ImageGeneration(image) = &incomplete[0] else {
        panic!("expected image generation item");
    };
    assert_eq!(image.status, "failed");
    assert!(lifecycle.take_incomplete().is_empty());
}

#[test]
fn grok_hosted_lifecycle_rejects_owner_changes() {
    let mut lifecycle = HostedToolLifecycle::default();
    lifecycle
        .record_added("shared-1", GrokHostedOutputOwner::WebSearch, "ws")
        .expect("first owner should be accepted");

    let hosted_change = lifecycle
        .record_completed("shared-1", GrokHostedOutputOwner::ImageGeneration, "ig")
        .expect_err("hosted owner change must fail");
    assert!(
        hosted_change
            .to_string()
            .contains("Web Search (ws) to Image Generation (ig)")
    );

    let local_change = lifecycle
        .record_completed("shared-1", GrokHostedOutputOwner::Ordinary, "fc")
        .expect_err("hosted-to-local owner change must fail");
    assert!(local_change.to_string().contains("ordinary local output"));

    let mut reverse = HostedToolLifecycle::default();
    reverse
        .record_added("shared-2", GrokHostedOutputOwner::Ordinary, "fc")
        .expect("ordinary owner should be recorded");
    let reverse_change = reverse
        .record_completed("shared-2", GrokHostedOutputOwner::XSearch, "ctc")
        .expect_err("local-to-hosted owner change must fail");
    assert!(
        reverse_change
            .to_string()
            .contains("ordinary local output (fc) to X Search (ctc)")
    );

    let mut ordinary_kind_change = HostedToolLifecycle::default();
    ordinary_kind_change
        .record_added("shared-3", GrokHostedOutputOwner::Ordinary, "msg")
        .expect("first ordinary kind should be recorded");
    let kind_change = ordinary_kind_change
        .record_completed("shared-3", GrokHostedOutputOwner::Ordinary, "fc")
        .expect_err("ordinary kind changes must fail");
    assert!(kind_change.to_string().contains("(msg)"));
    assert!(kind_change.to_string().contains("(fc)"));
}
