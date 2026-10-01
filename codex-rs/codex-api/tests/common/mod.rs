use codex_api::ResponsesApiRequest;
use codex_protocol::models::ResponseItem;

pub fn basic_request(input: Vec<ResponseItem>) -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "fixture-grok-model".into(),
        instructions: String::new(),
        input,
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream: true,
        stream_options: None,
        include: vec!["reasoning.encrypted_content".into()],
        service_tier: None,
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}
