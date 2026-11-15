use super::*;
use pretty_assertions::assert_eq;

fn signature_with_kind(kind: &[u8]) -> String {
    let mut bytes = vec![
        0x08, 0x04, 0x12, 0xf3, 0x06, 0x0a, 0x11, 0x08, 0x11, 0x18, 0x02, 0x38,
    ];
    bytes.extend_from_slice(&[0x01, 0x42, kind.len().try_into().expect("kind fits u8")]);
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&[0xab; 40]);
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn reasoning(summary: &str, signature: Option<&str>) -> ResponseItem {
    ResponseItem::Reasoning {
        id: None,
        summary: vec![ReasoningItemReasoningSummary::SummaryText {
            text: summary.to_string(),
        }],
        content: None,
        encrypted_content: signature.map(str::to_string),
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn narration_signature_becomes_commentary_and_thinking_does_not() {
    assert!(signature_marks_narration(&signature_with_kind(
        b"narration"
    )));
    assert!(!signature_marks_narration(&signature_with_kind(
        b"thinking"
    )));
    assert!(!signature_marks_narration(""));
    assert!(!signature_marks_narration("tco_not_a_signature"));

    let narration = commentary_turn_item(&reasoning(
        "Found the bug.",
        Some(&signature_with_kind(b"narration")),
    ))
    .expect("narration");
    let TurnItem::AgentMessage(message) = narration else {
        panic!("expected commentary message");
    };
    assert_eq!(message.phase, Some(MessagePhase::Commentary));
    assert!(message.id.starts_with("msg_"));
    let [AgentMessageContent::Text { text }] = message.content.as_slice() else {
        panic!("expected one narration text part");
    };
    assert_eq!(text, "Found the bug.");

    assert!(
        commentary_turn_item(&reasoning(
            "internal reasoning",
            Some(&signature_with_kind(b"thinking")),
        ))
        .is_none()
    );
    assert!(commentary_turn_item(&reasoning("Found the bug.", None)).is_none());
    assert!(
        commentary_turn_item(&reasoning("", Some(&signature_with_kind(b"narration")))).is_none()
    );
}
