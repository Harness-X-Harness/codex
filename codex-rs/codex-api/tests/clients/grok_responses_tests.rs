use super::*;
use codex_api::Reasoning;
use codex_api::ReasoningContext;
use codex_api::create_text_param_for_request;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::json;

fn grok_client(state: &RecordingState) -> ResponsesClient<RecordingTransport> {
    ResponsesClient::new(
        RecordingTransport::new(state.clone()),
        provider("openai"),
        Arc::new(NoAuth),
    )
    .with_dialect(ApiDialect::Grok)
}

#[tokio::test]
async fn explicit_grok_dialect_projects_only_supported_request_fields() -> Result<()> {
    let state = RecordingState::default();
    let client = grok_client(&state).with_telemetry(/*request*/ None, /*sse*/ None);
    let mut request = common::basic_request(vec![serde_json::from_value(json!({
        "type": "message", "id": "msg_1", "role": "user",
        "content": [{"type": "input_text", "text": "hi"}],
        "phase": "commentary",
        "internal_chat_message_metadata_passthrough": {"turn_id": "private-turn"}
    }))?]);
    request.instructions = "Say hi".into();
    request.service_tier = Some("priority".into());
    request.prompt_cache_key = Some("cache-key".into());
    request.client_metadata = Some([("private_key".into(), "private_value".into())].into());
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

#[tokio::test]
async fn grok_replays_reasoning_without_plaintext_beside_any_encrypted_field() -> Result<()> {
    let input = json!([
        {"type":"reasoning", "id":"encrypted", "summary":[], "encrypted_content":"cipher", "content":[{"type":"reasoning_text", "text":"private"}], "internal_chat_message_metadata_passthrough":{"turn_id":"private-turn"}},
        {"type":"reasoning", "id":"empty", "summary":[], "encrypted_content":"", "content":[{"type":"reasoning_text", "text":"private"}]},
        {"type":"reasoning", "id":"plain", "summary":[{"type":"summary_text", "text":"short"}], "content":[{"type":"reasoning_text", "text":"public"}]},
        {"type":"message", "role":"assistant", "phase":"final_answer", "content":[{"type":"output_text", "text":"answer"}]}
    ]);
    let mut request = common::basic_request(serde_json::from_value(input)?);
    request.reasoning = Some(Reasoning {
        effort: Some(ReasoningEffort::Custom("64".into())),
        summary: Some(ReasoningSummary::Detailed),
        context: Some(ReasoningContext::AllTurns),
    });
    request.text = create_text_param_for_request(
        Some(Verbosity::High),
        &Some(json!({"type":"string"})),
        /*output_schema_strict*/ true,
    );
    request.tools = Some(empty_tools().into());
    let state = RecordingState::default();
    let client = grok_client(&state);
    client
        .stream_request(request, ResponsesOptions::default())
        .await?;
    let requests = state.take_stream_requests();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request_body_bytes(&requests[0]))?,
        json!({
            "model":"fixture-grok-model", "input":[
                {"type":"reasoning", "id":"encrypted", "summary":[], "encrypted_content":"cipher"},
                {"type":"reasoning", "id":"empty", "summary":[], "encrypted_content":""},
                {"type":"reasoning", "id":"plain", "summary":[{"type":"summary_text", "text":"short"}], "content":[{"type":"reasoning_text", "text":"public"}]},
                {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"answer"}]}
            ],
            "reasoning":{"effort":64,"summary":"detailed"}, "stream":true, "include":["reasoning.encrypted_content"],
            "text":{"format":{"type":"json_schema","strict":true,"schema":{"type":"string"},"name":"codex_output_schema"}}
        })
    );
    Ok(())
}

#[tokio::test]
async fn default_stock_dialect_ignores_grok_name_and_destination() -> Result<()> {
    let state = RecordingState::default();
    let mut destination = provider("grok");
    destination.base_url = "https://api.x.ai/v1".into();
    let client = ResponsesClient::new(
        RecordingTransport::new(state.clone()),
        destination,
        Arc::new(NoAuth),
    );
    client
        .stream_request(common::basic_request(vec![]), ResponsesOptions::default())
        .await?;
    let requests = state.take_stream_requests();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request_body_bytes(&requests[0]))?,
        json!({
            "model":"fixture-grok-model", "input":[], "tool_choice":"auto", "parallel_tool_calls":true,
            "reasoning":null,"store":false,"stream":true,"include":["reasoning.encrypted_content"]
        })
    );
    Ok(())
}

#[tokio::test]
async fn grok_rejects_unsupported_history_tools_and_raw_json_before_transport() -> Result<()> {
    let state = RecordingState::default();
    let client = grok_client(&state);
    for item in [
        json!({"type":"message","role":"user","content":[{"type":"input_image","file_id":"file_1"}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_audio","audio_url":"data:audio"}]}),
        json!({"type":"function_call","name":"f","arguments":"{}","call_id":"c","namespace":"ns","encrypted_function_args":["cipher"]}),
        json!({"type":"agent_message","author":"child","recipient":"parent","content":[{"type":"encrypted_content","encrypted_content":"cipher"}]}),
        json!({"type":"compaction","encrypted_content":"cipher"}),
        json!({"type":"reasoning","summary":[],"content":[{"type":"text","text":"untyped"}]}),
        json!({"type":"future_item"}),
    ] {
        let request = common::basic_request(vec![serde_json::from_value(item)?]);
        assert!(matches!(
            client
                .stream_request(request, ResponsesOptions::default())
                .await,
            Err(ApiError::Stream(_))
        ));
    }
    let mut request = common::basic_request(vec![]);
    let tools: Arc<RawValue> = Arc::from(RawValue::from_string(
        r#"[{"type":"namespace","name":"ns","tools":[]}]"#.into(),
    )?);
    request.tools = Some(tools.into());
    assert!(matches!(
        client
            .stream_request(request, ResponsesOptions::default())
            .await,
        Err(ApiError::Stream(_))
    ));
    assert!(matches!(
        client
            .stream(
                json!({"model":"bypass"}),
                HeaderMap::new(),
                Compression::None,
                /*turn_state*/ None
            )
            .await,
        Err(ApiError::Stream(_))
    ));
    assert!(state.take_stream_requests().is_empty());
    Ok(())
}

#[tokio::test]
async fn grok_local_tool_history_reaches_transport_with_exact_arguments() -> Result<()> {
    let state = RecordingState::default();
    let client = grok_client(&state);
    let arguments = "{ \"value\" : 9007199254740993 }\n";
    let input = json!([
        {"type":"function_call", "name":"functions__shell", "arguments":arguments,
         "call_id":"call_1"},
        {"type":"function_call_output", "call_id":"call_1", "output":"done"}
    ]);
    let mut request = common::basic_request(serde_json::from_value(input.clone())?);
    let tools: Arc<RawValue> = Arc::from(RawValue::from_string(
        r#"[{"type":"function","name":"functions__shell","parameters":{},"strict":false}]"#.into(),
    )?);
    request.tools = Some(tools.into());
    client
        .stream_request(request, ResponsesOptions::default())
        .await?;
    let requests = state.take_stream_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request_body_bytes(&requests[0]))?,
        json!({
            "model":"fixture-grok-model", "input":input,
            "tools":[{"type":"function","name":"functions__shell","parameters":{}}],
            "tool_choice":"auto", "reasoning":null, "stream":true,
            "include":["reasoning.encrypted_content"]
        })
    );
    Ok(())
}
