use codex_protocol::ResponseItemId;
use codex_protocol::items::ImageGenerationItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseItem;

use super::HostedToolCompletion;
use super::HostedToolEventPhase;
use super::HostedToolLifecycle;
use super::validate_grok_hosted_item;

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

    assert!(lifecycle.record_started("provider-image-1", item.clone(), true));
    assert!(!lifecycle.record_started("provider-image-1", item.clone(), true));
    let HostedToolCompletion::First {
        started_item: Some(started),
    } = lifecycle.record_completed("provider-image-1")
    else {
        panic!("expected the first hosted completion");
    };
    assert_eq!(started.id(), item.id());
    assert!(matches!(
        lifecycle.record_completed("provider-image-1"),
        HostedToolCompletion::Duplicate
    ));
}

#[test]
fn grok_hosted_lifecycle_reports_a_completion_without_an_observed_start() {
    let mut lifecycle = HostedToolLifecycle::default();

    assert!(matches!(
        lifecycle.record_completed("web-1"),
        HostedToolCompletion::First { started_item: None }
    ));
    assert!(lifecycle.has_seen("web-1"));
}

#[test]
fn grok_incomplete_hosted_items_fail_in_memory_without_creating_a_durable_item() {
    let mut lifecycle = HostedToolLifecycle::default();
    assert!(lifecycle.record_started("image-1", image_item("image-1", "in_progress"), true));

    let incomplete = lifecycle.take_incomplete();
    assert_eq!(incomplete.len(), 1);
    let TurnItem::ImageGeneration(image) = &incomplete[0] else {
        panic!("expected image generation item");
    };
    assert_eq!(image.status, "failed");
    assert!(lifecycle.take_incomplete().is_empty());
}

#[test]
fn grok_hosted_event_validation_requires_provider_identity_and_monotonic_status() {
    let valid = ResponseItem::CustomToolCall {
        id: Some(ResponseItemId::with_suffix("ct", "x")),
        status: Some("completed".to_string()),
        call_id: "call-x".to_string(),
        name: "x_keyword_search".to_string(),
        namespace: None,
        input: r#"{"query":"Codex"}"#.to_string(),
        internal_chat_message_metadata_passthrough: None,
    };
    assert_eq!(
        validate_grok_hosted_item(&valid, true, HostedToolEventPhase::Done),
        Ok(Some("ct_x"))
    );

    let mut missing_id = valid.clone();
    missing_id.set_id(None);
    assert_eq!(
        validate_grok_hosted_item(&missing_id, true, HostedToolEventPhase::Done),
        Err("hosted output is missing provider item_id")
    );

    assert_eq!(
        validate_grok_hosted_item(&valid, true, HostedToolEventPhase::Added),
        Err("hosted start has an invalid status")
    );
    assert_eq!(
        validate_grok_hosted_item(&valid, false, HostedToolEventPhase::Done),
        Ok(None),
        "an undeclared CustomToolCall is not hosted X Search"
    );
}
