use super::*;
use pretty_assertions::assert_eq;

/// The Grok Responses dialect drops the tool-control fields from a request that
/// declares no tools, because the verified Grok backend rejects an empty tool
/// declaration together with `tool_choice` / `parallel_tool_calls`.
#[tokio::test]
async fn grok_no_tool_request_omits_tool_controls() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let mut grok = provider("grok");
    grok.responses_dialect = codex_api::ResponsesDialect::Grok;
    let client = ResponsesClient::new(transport, grok, Arc::new(NoAuth));
    let request = ResponsesApiRequest {
        model: "grok-test".into(),
        instructions: "Say hi".into(),
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".into(),
            content: vec![ContentItem::InputText { text: "hi".into() }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }],
        tools: Some(empty_tools().into()),
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
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
    };

    let _stream = client
        .stream_request(request, ResponsesOptions::default())
        .await?;

    let requests = state.take_stream_requests();
    assert_eq!(requests.len(), 1);
    let prepared = requests[0]
        .prepare_body_for_send()
        .expect("body should prepare");
    let body: serde_json::Value =
        serde_json::from_slice(prepared.body.as_deref().expect("body should be JSON"))?;
    assert_eq!(body["model"], "grok-test");
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("parallel_tool_calls").is_none());
    Ok(())
}
