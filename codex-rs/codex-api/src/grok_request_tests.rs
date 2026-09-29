use super::build;
use crate::common::ResponsesApiRequest;
use pretty_assertions::assert_eq;
use serde_json::json;

fn request() -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "grok-test".to_string(),
        instructions: "system".to_string(),
        input: Vec::new(),
        tools: None,
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream: true,
        stream_options: None,
        include: vec!["reasoning.encrypted_content".to_string()],
        service_tier: Some("flex".to_string()),
        prompt_cache_key: Some("cache-key".to_string()),
        text: None,
        client_metadata: Some(std::collections::HashMap::from([(
            "client".to_string(),
            "metadata".to_string(),
        )])),
        access_programs: None,
    }
}

#[test]
fn basic_projection_is_whitelist_based() {
    let body = build(&request()).expect("Grok request should project");

    assert_eq!(body["model"], json!("grok-test"));
    assert_eq!(body["instructions"], json!("system"));
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["prompt_cache_key"], json!("cache-key"));

    assert!(body.get("parallel_tool_calls").is_none());
    assert!(body.get("store").is_none());
    assert!(body.get("stream_options").is_none());
    assert!(body.get("service_tier").is_none());
    assert!(body.get("client_metadata").is_none());
    assert!(body.get("access_programs").is_none());
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn empty_instructions_are_omitted() {
    let mut request = request();
    request.instructions.clear();
    let body = build(&request).expect("Grok request should project");
    assert!(body.get("instructions").is_none());
}
