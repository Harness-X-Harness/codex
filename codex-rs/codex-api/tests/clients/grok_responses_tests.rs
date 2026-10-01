use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn explicit_grok_dialect_projects_only_supported_request_fields() -> Result<()> {
    let state = RecordingState::default();
    let client = ResponsesClient::new(
        RecordingTransport::new(state.clone()),
        provider("openai"),
        Arc::new(NoAuth),
    )
    .with_dialect(ApiDialect::Grok)
    .with_telemetry(/*request*/ None, /*sse*/ None);
    let request = ResponsesApiRequest {
        model: "fixture-grok-model".into(),
        instructions: "Say hi".into(),
        input: vec![serde_json::from_value(json!({
            "type": "message", "id": "msg_1", "role": "user",
            "content": [{"type": "input_text", "text": "hi"}],
            "phase": "commentary",
            "internal_chat_message_metadata_passthrough": {"turn_id": "private-turn"}
        }))?],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream: true,
        stream_options: None,
        include: vec!["reasoning.encrypted_content".into()],
        service_tier: Some("priority".into()),
        prompt_cache_key: Some("cache-key".into()),
        text: None,
        client_metadata: Some([("private_key".into(), "private_value".into())].into()),
        access_programs: None,
    };
    client
        .stream_request(request, ResponsesOptions::default())
        .await?;
    let requests = state.take_stream_requests();
    assert_path_ends_with(&requests, "/responses");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request_body_bytes(&requests[0]))?,
        json!({
            "model": "fixture-grok-model", "instructions": "Say hi",
            "input": [{"type": "message", "id": "msg_1", "role": "user",
                "content": [{"type": "input_text", "text": "hi"}]}],
            "reasoning": null, "stream": true,
            "include": ["reasoning.encrypted_content"], "prompt_cache_key": "cache-key"
        })
    );
    Ok(())
}
