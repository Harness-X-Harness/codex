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
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/api/codex/responses"))
        .respond_with(|request: &wiremock::Request| {
            let body = request
                .body_json::<serde_json::Value>()
                .expect("JSON request");
            let is_edit = body.to_string().contains("Edit the prior image");
            let (call_id, response_id, arguments) = if is_edit {
                (
                    "edit-1",
                    "resp-edit",
                    r#"{"prompt":"add a red hat","num_last_images_to_include":1}"#,
                )
            } else {
                (
                    "generate-1",
                    "resp-generate",
                    r#"{"prompt":"paint a blue whale"}"#,
                )
            };
            let has_output = body["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "function_call_output" && item["call_id"] == call_id
                })
            });
            let events = if has_output {
                vec![
                    responses::ev_assistant_message(&format!("msg-{call_id}"), "Done"),
                    responses::ev_completed(&format!("reply-{call_id}")),
                ]
            } else {
                let imagegen_wire_name = body["tools"]
                    .as_array()
                    .and_then(|tools| {
                        tools.iter().find(|tool| {
                            tool["type"] == "function"
                                && tool["description"].as_str().is_some_and(|description| {
                                    description.contains("canonical `image_gen.imagegen` tool")
                                })
                        })
                    })
                    .and_then(|tool| tool["name"].as_str())
                    .expect("request should declare the flat image function");
                vec![
                    responses::ev_response_created(response_id),
                    responses::ev_function_call(call_id, imagegen_wire_name, arguments),
                    responses::ev_completed(response_id),
                ]
            };
            responses::sse_response(responses::sse(events))
        })
        .mount(&server)
        .await;

    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model("grok-4.6")
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
    assert!(bodies.iter().any(|body| body == &json!({"model":"grok-imagine-image-2.0","prompt":"paint a blue whale","response_format":"b64_json"})));
    assert!(bodies.iter().any(|body| body == &json!({"model":"grok-imagine-image-2.0","prompt":"add a red hat","response_format":"b64_json","image":{"type":"image_url","url":format!("data:image/jpeg;base64,{encoded}")}})));
    let response_bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(wiremock::Request::body_json::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let generation_body = response_bodies
        .iter()
        .find(|body| body.to_string().contains("Generate an image"))
        .context("generation request should reach Grok")?;
    let generation_tools = generation_body
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .context("generation request should declare tools")?
        .clone();
    let imagegen_tool = generation_tools
        .iter()
        .find(|tool| {
            tool.get("type").and_then(serde_json::Value::as_str) == Some("function")
                && tool
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|description| {
                        description.contains("canonical `image_gen.imagegen` tool")
                    })
        })
        .context("generation request should declare the flat image function")?;
    let description = imagegen_tool
        .get("description")
        .and_then(serde_json::Value::as_str)
        .context("flat image function should retain its model guidance")?;
    assert!(description.contains("canonical `image_gen.imagegen` tool"));
    assert!(description.contains("Call this function itself."));
    assert!(description.contains(
        "The `image_gen.imagegen` tool enables image generation from descriptions and editing of existing images"
    ));
    assert!(description.contains("The current tool configuration accepts at most 3 edit images."));
    response_bodies
        .iter()
        .find(|body| body.to_string().contains("Edit the prior image"))
        .context("history-edit request should reach Grok")?;
    Ok(())
}
