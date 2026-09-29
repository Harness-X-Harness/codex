use super::build;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use crate::provider::XSearchProviderConfig;
use pretty_assertions::assert_eq;
use serde_json::json;
use serde_json::value::RawValue;
use std::sync::Arc;

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
    let body = build(&request(), None).expect("Grok request should project");

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
    let body = build(&request, None).expect("Grok request should project");
    assert!(body.get("instructions").is_none());
}


fn raw_tools(value: serde_json::Value) -> ResponsesApiTools {
    let raw = RawValue::from_string(value.to_string()).expect("valid raw tool JSON");
    ResponsesApiTools::from(Arc::<RawValue>::from(raw))
}

#[test]
fn provider_x_search_window_applies_to_appended_tool() {
    let mut request = request();
    request.tools = Some(raw_tools(json!([{"type": "web_search"}])));
    let window = XSearchProviderConfig {
        from_date: Some("2026-01-01".to_string()),
        to_date: Some("2026-01-31".to_string()),
    };
    let body = build(&request, Some(&window)).expect("Grok request should project");
    let x_search = body["tools"].as_array().unwrap().iter()
        .find(|tool| tool["type"] == "x_search").unwrap();
    assert_eq!(x_search, &json!({
        "type": "x_search",
        "from_date": "2026-01-01",
        "to_date": "2026-01-31"
    }));
}

#[test]
fn explicit_x_search_dates_override_provider_defaults_individually() {
    let mut request = request();
    request.tools = Some(raw_tools(json!([
        {"type": "x_search", "from_date": "2026-02-01"}
    ])));
    let window = XSearchProviderConfig {
        from_date: Some("2026-01-01".to_string()),
        to_date: Some("2026-02-28".to_string()),
    };
    let body = build(&request, Some(&window)).expect("Grok request should project");
    assert_eq!(body["tools"][0], json!({
        "type": "x_search",
        "from_date": "2026-02-01",
        "to_date": "2026-02-28"
    }));
}
