use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn grok_image_generation_then_history_edit_uses_stock_lifecycle() -> Result<()> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let jpeg = include_bytes!("../../../../vendor/bubblewrap/bubblewrap.jpg");
    let encoded = STANDARD.encode(jpeg);
    let server = responses::start_mock_server().await;
    for endpoint in ["generations", "edits"] {
        Mock::given(method("POST"))
            .and(path(format!("/api/codex/images/{endpoint}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"b64_json": encoded, "mime_type": "image/jpeg"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let calls = [
        ("resp-1", "generate-1", r#"{"prompt":"paint a blue whale"}"#),
        (
            "resp-3",
            "edit-1",
            r#"{"prompt":"add a red hat","num_last_images_to_include":1}"#,
        ),
    ];
    let mut sequence = Vec::new();
    for (index, (response_id, call_id, arguments)) in calls.into_iter().enumerate() {
        sequence.push(responses::sse(vec![
            responses::ev_response_created(response_id),
            responses::ev_function_call_with_namespace(call_id, "image_gen", "imagegen", arguments),
            responses::ev_completed(response_id),
        ]));
        sequence.push(responses::sse(vec![
            responses::ev_assistant_message(&format!("msg-{index}"), "Done"),
            responses::ev_completed(&format!("reply-{index}")),
        ]));
    }
    let response_mock = responses::mount_sse_sequence(&server, sequence).await;

    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model_provider("grok")
        .with_provider_name("Grok")
        .with_provider_base_url(&format!("{}/api/codex", server.uri()))
        .with_grok_responses_wire_api()
        .with_provider_config("supports_websockets = false\nrequires_openai_auth = false")
        .write(codex_home.path())?;
    write_chatgpt_auth(
        codex_home.path(),
        ChatGptAuthFixture::new("access-chatgpt"),
        AuthCredentialsStoreMode::File,
    )?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;
    let request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, app.read_response(request)).await??;
    for prompt in ["Generate an image", "Edit the prior image"] {
        let request = app
            .send_turn_start_request(TurnStartParams {
                thread_id: thread.id.clone(),
                input: vec![V2UserInput::Text {
                    text: prompt.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        let _: TurnStartResponse =
            timeout(DEFAULT_READ_TIMEOUT, app.read_response(request)).await??;
        let completed = timeout(
            DEFAULT_READ_TIMEOUT,
            wait_for_image_generation_completed(&mut app),
        )
        .await??;
        let ThreadItem::ImageGeneration(item) = completed.item else {
            panic!("expected image generation item");
        };
        let saved = item.saved_path.context("image should be saved")?;
        assert_eq!(
            saved.extension().and_then(|value| value.to_str()),
            Some("jpg")
        );
        assert_eq!(std::fs::read(saved)?, jpeg);
        timeout(
            DEFAULT_READ_TIMEOUT,
            app.read_stream_until_notification_message("turn/completed"),
        )
        .await??;
    }

    let requests = server.received_requests().await.context("requests")?;
    let bodies = requests
        .iter()
        .filter(|request| request.url.path().contains("/images/"))
        .map(wiremock::Request::body_json::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        bodies,
        vec![
            json!({"model":"grok-imagine-image-2.0","prompt":"paint a blue whale","response_format":"b64_json"}),
            json!({"model":"grok-imagine-image-2.0","prompt":"add a red hat","response_format":"b64_json","image":{"type":"image_url","url":format!("data:image/jpeg;base64,{encoded}")}}),
        ]
    );
    let responses = response_mock.requests();
    assert_eq!(responses.len(), 4);
    assert!(responses[0].body_contains_text("Generate an image"));
    assert!(responses[0].tool_by_name("image_gen", "imagegen").is_some());
    assert!(responses[2].body_contains_text("Edit the prior image"));
    Ok(())
}
