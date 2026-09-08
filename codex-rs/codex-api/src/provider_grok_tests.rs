//! Grok Responses dialect contracts, kept apart from the stock `provider`
//! test module so upstream edits there never collide with the Grok graft.

use crate::common::AccessPrograms;
use crate::common::Reasoning;
use crate::common::ReasoningContext;
use crate::common::ReasoningSummaryDelivery;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::common::StreamOptions;
use crate::common::TextControls;
use crate::provider::ResponsesDialect;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::turn_input::CyberAccessProgram;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use serde_json::value::RawValue;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

fn raw_tools(tools: Value) -> ResponsesApiTools {
    ResponsesApiTools::from(Arc::<RawValue>::from(
        RawValue::from_string(tools.to_string()).expect("valid tool declaration"),
    ))
}

fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn responses_request_with_tools(tools: Value) -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "grok-test".to_string(),
        instructions: "test".to_string(),
        input: vec![user_message("test")],
        tools: Some(raw_tools(tools)),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        reasoning: None,
        store: false,
        stream: true,
        stream_options: None,
        include: Vec::new(),
        service_tier: None,
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

/// Every stock `ResponsesApiRequest` field set to a serializing value. A new
/// upstream field fails to compile here, forcing a decision about whether the
/// Grok gateway accepts it or the dialect must strip it.
fn fully_populated_request() -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "grok-test".to_string(),
        instructions: "test".to_string(),
        input: vec![user_message("test")],
        tools: Some(raw_tools(
            json!([{"type": "web_search", "external_web_access": true}]),
        )),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
        reasoning: Some(Reasoning {
            effort: Some(ReasoningEffort::High),
            summary: Some(ReasoningSummary::Auto),
            context: Some(ReasoningContext::Auto),
        }),
        store: false,
        stream: true,
        stream_options: Some(StreamOptions {
            reasoning_summary_delivery: ReasoningSummaryDelivery::SequentialCutoff,
        }),
        include: vec!["reasoning.encrypted_content".to_string()],
        service_tier: Some("default".to_string()),
        prompt_cache_key: Some("thread".to_string()),
        text: Some(TextControls {
            verbosity: None,
            format: None,
        }),
        client_metadata: Some(HashMap::from([("k".to_string(), "v".to_string())])),
        access_programs: Some(AccessPrograms::from(CyberAccessProgram::Standard)),
    }
}

#[test]
fn grok_projection_forwards_only_reviewed_request_fields() {
    let projected = ResponsesDialect::Grok
        .project_request(&fully_populated_request())
        .expect("fully populated request should project");

    let forwarded = projected
        .as_object()
        .expect("projected request is an object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    // Reviewed against the Grok Responses gateway: these top-level fields are
    // forwarded unchanged. Extend this list only after re-proving the Live
    // `basic-exact-reply` scenario with the new field present.
    assert_eq!(
        forwarded,
        BTreeSet::from([
            "access_programs",
            "client_metadata",
            "include",
            "input",
            "instructions",
            "model",
            "parallel_tool_calls",
            "prompt_cache_key",
            "reasoning",
            "service_tier",
            "store",
            "stream",
            "stream_options",
            "text",
            "tool_choice",
            "tools",
        ])
    );
}

#[test]
fn grok_projects_verified_live_web_search_to_bare_declaration() {
    let request = responses_request_with_tools(json!([{
        "type": "web_search",
        "external_web_access": true,
    }]));

    let projected = ResponsesDialect::Grok
        .project_request(&request)
        .expect("verified live search should project");

    assert_eq!(projected["tools"], json!([{"type": "web_search"}]));
}

#[test]
fn grok_rejects_unverified_web_search_declarations() {
    for tool in [
        json!({"type": "web_search", "external_web_access": false}),
        json!({
            "type": "web_search",
            "external_web_access": true,
            "indexed_web_access": true,
        }),
        json!({
            "type": "web_search",
            "external_web_access": true,
            "search_context_size": "high",
        }),
    ] {
        let request = responses_request_with_tools(json!([tool]));

        assert!(ResponsesDialect::Grok.project_request(&request).is_err());
    }
}

#[test]
fn grok_rejects_residual_agent_message_before_transport() {
    let mut request = responses_request_with_tools(json!([]));
    request.input = vec![ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/child".to_string(),
        content: vec![AgentMessageInputContent::EncryptedContent {
            encrypted_content: "opaque".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    }];

    assert!(ResponsesDialect::Grok.project_request(&request).is_err());
}
