use super::*;
use codex_protocol::ResponseItemId;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn plaintext_agent_message_projects_to_standard_user_message() {
    let input = vec![ResponseItem::AgentMessage {
        id: Some(ResponseItemId::with_suffix("amsg", "child")),
        author: "/root".to_string(),
        recipient: "/root/reviewer".to_string(),
        content: vec![AgentMessageInputContent::InputText {
            text:
                "Message Type: NEW_TASK\nTask name: /root/reviewer\nSender: /root\nPayload:\nreview"
                    .to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    }];

    let encoded = project(input).expect("plaintext collaboration history should project");

    assert_eq!(
        encoded,
        vec![ResponseItem::Message {
            id: Some(ResponseItemId::with_suffix("amsg", "child")),
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Message Type: NEW_TASK\nTask name: /root/reviewer\nSender: /root\nPayload:\nreview".to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn encrypted_agent_message_fails_before_provider_request() {
    let input = vec![ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/reviewer".to_string(),
        content: vec![AgentMessageInputContent::EncryptedContent {
            encrypted_content: "opaque".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    }];

    assert_eq!(
        project(input)
            .expect_err("encrypted history must fail")
            .to_string(),
        "Grok cannot replay encrypted collaboration history"
    );
}

#[test]
fn unsupported_codex_only_history_fails_before_provider_request() {
    let input = vec![ResponseItem::ToolSearchCall {
        id: None,
        call_id: Some("search-1".to_string()),
        status: Some("completed".to_string()),
        execution: "client".to_string(),
        arguments: serde_json::json!({"query": "calendar"}),
        internal_chat_message_metadata_passthrough: None,
    }];

    assert_eq!(
        project(input)
            .expect_err("unsupported history must fail")
            .to_string(),
        "Grok does not support Codex history item `tool_search_call`"
    );
}

#[test]
fn image_history_uses_native_grok_wire_shape() {
    let input = vec![ResponseItem::GrokImageGenerationCall {
        id: Some(ResponseItemId::with_suffix("ig", "123")),
        status: "failed".to_string(),
        prompt: Some("Draw a fox.".to_string()),
        result: None,
        internal_chat_message_metadata_passthrough: None,
    }];

    let encoded = project(input).expect("Grok image history should project");

    assert_eq!(
        serde_json::to_value(encoded).expect("request history should serialize"),
        json!([{
            "id": "ig_123",
            "type": "image_generation_call",
            "status": "failed",
            "prompt": "Draw a fox."
        }])
    );
}

#[test]
fn x_search_history_replays_exactly() {
    let input = vec![ResponseItem::CustomToolCall {
        id: Some(ResponseItemId::with_suffix("ct", "x")),
        status: Some("completed".to_string()),
        call_id: "call_x".to_string(),
        name: "x_thread_fetch".to_string(),
        namespace: None,
        input: r#"{"post_id":"123"}"#.to_string(),
        internal_chat_message_metadata_passthrough: None,
    }];

    let encoded = project(input).expect("X Search history should pass through");

    assert_eq!(
        serde_json::to_value(encoded).expect("X Search history should serialize"),
        json!([{
            "id": "ct_x",
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": "call_x",
            "name": "x_thread_fetch",
            "input": "{\"post_id\":\"123\"}"
        }])
    );
}

#[test]
fn null_reasoning_content_uses_gateway_accepted_shape() {
    let input = vec![ResponseItem::Reasoning {
        id: Some(ResponseItemId::with_suffix("rs", "123")),
        summary: Vec::new(),
        content: None,
        encrypted_content: Some("opaque".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }];

    let encoded = project(input).expect("Grok reasoning history should normalize");

    assert_eq!(
        serde_json::to_value(encoded).expect("reasoning history should serialize"),
        json!([{
            "id": "rs_123",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "opaque"
        }])
    );
}
