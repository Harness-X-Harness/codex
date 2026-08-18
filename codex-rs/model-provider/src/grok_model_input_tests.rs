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
        encoded.into_items(),
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
fn tool_search_history_projects_to_ordinary_function_pair_only_on_the_wire() {
    let input = vec![
        ResponseItem::ToolSearchCall {
            id: None,
            call_id: Some("search-1".to_string()),
            status: Some("completed".to_string()),
            execution: "client".to_string(),
            arguments: json!({"query": "calendar"}),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::ToolSearchOutput {
            id: None,
            call_id: Some("search-1".to_string()),
            status: "completed".to_string(),
            execution: "client".to_string(),
            tools: vec![json!({"type": "function", "name": "calendar_create"})],
            internal_chat_message_metadata_passthrough: None,
        },
    ];

    let projected = project(input.clone()).expect("tool_search history should project");

    assert_eq!(projected.clone().into_items(), input);
    assert_eq!(
        serde_json::to_value(projected).expect("projected request should serialize"),
        json!([
            {
                "type": "function_call",
                "name": "tool_search",
                "arguments": "{\"query\":\"calendar\"}",
                "call_id": "search-1"
            },
            {
                "type": "function_call_output",
                "call_id": "search-1",
                "output": "{\"tools\":[{\"name\":\"calendar_create\",\"type\":\"function\"}]}"
            }
        ])
    );
}

#[test]
fn incomplete_or_non_client_tool_search_history_fails_before_provider_request() {
    let missing_call_id = ResponseItem::ToolSearchCall {
        id: None,
        call_id: None,
        status: Some("completed".to_string()),
        execution: "client".to_string(),
        arguments: json!({"query": "calendar"}),
        internal_chat_message_metadata_passthrough: None,
    };
    assert_eq!(
        project(vec![missing_call_id])
            .expect_err("missing call id must fail")
            .to_string(),
        "Grok tool_search history is missing its call id"
    );

    let server_output = ResponseItem::ToolSearchOutput {
        id: None,
        call_id: Some("search-1".to_string()),
        status: "completed".to_string(),
        execution: "server".to_string(),
        tools: Vec::new(),
        internal_chat_message_metadata_passthrough: None,
    };
    assert_eq!(
        project(vec![server_output])
            .expect_err("server tool_search output must not use the client projection")
            .to_string(),
        "Grok can only replay completed client tool_search outputs"
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
fn completed_image_history_omits_result_only_from_request_projection() {
    let durable_result = "opaque-image-result".repeat(100);
    let input = vec![ResponseItem::GrokImageGenerationCall {
        id: Some(ResponseItemId::with_suffix("ig", "123")),
        status: "completed".to_string(),
        prompt: Some("Draw a fox.".to_string()),
        result: Some(durable_result.clone()),
        internal_chat_message_metadata_passthrough: None,
    }];

    let projected = project(input).expect("completed Grok image history should project");

    assert_eq!(
        serde_json::to_value(&projected).expect("request history should serialize"),
        json!([{
            "id": "ig_123",
            "type": "image_generation_call",
            "status": "completed",
            "prompt": "Draw a fox."
        }])
    );
    assert_eq!(
        projected.into_items(),
        vec![ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "123")),
            status: "completed".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: Some(durable_result),
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn image_history_preserves_the_exact_provider_item_id() {
    let input = vec![ResponseItem::GrokImageGenerationCall {
        id: Some(ResponseItemId::from_server("provider-image-id".to_string())),
        status: "completed".to_string(),
        prompt: Some("Draw a fox.".to_string()),
        result: Some("opaque-image-result".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }];

    let projected = project(input.clone()).expect("completed Grok image history should project");

    assert_eq!(
        serde_json::to_value(&projected).expect("request history should serialize"),
        json!([{
            "id": "provider-image-id",
            "type": "image_generation_call",
            "status": "completed",
            "prompt": "Draw a fox."
        }])
    );
    assert_eq!(projected.into_items(), input);
}

#[test]
fn multiple_completed_images_preserve_order_and_omit_each_wire_result() {
    let input = vec![
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "first")),
            status: "completed".to_string(),
            prompt: Some("First".to_string()),
            result: Some("first-result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "second")),
            status: "completed".to_string(),
            prompt: Some("Second".to_string()),
            result: Some("second-result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
    ];

    let projected = project(input.clone()).expect("multiple Grok images should project");

    assert_eq!(
        serde_json::to_value(&projected).expect("request history should serialize"),
        json!([
            {
                "id": "ig_first",
                "type": "image_generation_call",
                "status": "completed",
                "prompt": "First"
            },
            {
                "id": "ig_second",
                "type": "image_generation_call",
                "status": "completed",
                "prompt": "Second"
            }
        ])
    );
    assert_eq!(projected.into_items(), input);
}

#[test]
fn image_history_requires_the_evidence_backed_terminal_shape() {
    for item in [
        ResponseItem::GrokImageGenerationCall {
            id: None,
            status: "completed".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: Some("result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::from_server(String::new())),
            status: "completed".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: Some("result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "prompt")),
            status: "completed".to_string(),
            prompt: None,
            result: Some("result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "status")),
            status: "in_progress".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: None,
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix(
                "ig",
                "completed-without-result",
            )),
            status: "completed".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: None,
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::GrokImageGenerationCall {
            id: Some(ResponseItemId::with_suffix("ig", "failed-with-result")),
            status: "failed".to_string(),
            prompt: Some("Draw a fox.".to_string()),
            result: Some("unexpected-result".to_string()),
            internal_chat_message_metadata_passthrough: None,
        },
    ] {
        project(vec![item]).expect_err("incomplete image history must fail before egress");
    }
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
