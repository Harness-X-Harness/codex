use super::*;
use codex_protocol::models::FunctionCallOutputPayload;
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn request(tools: Value, input: Value) -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "grok-test".into(),
        instructions: String::new(),
        input: serde_json::from_value(input).unwrap(),
        tools: Some(
            Arc::<serde_json::value::RawValue>::from(
                serde_json::value::to_raw_value(&tools).unwrap(),
            )
            .into(),
        ),
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: true,
        stream: true,
        stream_options: None,
        include: Vec::new(),
        service_tier: Some("priority".into()),
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

#[test]
fn local_tools_use_only_the_verified_wire_fields() {
    let arguments = "{ \"n\" : 9007199254740993, \"text\": \"\\u0061\" }\n";
    let input = json!([
        {"type": "function_call", "id": "fc_1", "call_id": "call_1",
         "name": "functions__shell", "arguments": arguments},
        {"type": "function_call_output", "id": "out_1", "call_id": "call_1",
         "name": "functions__shell", "output": "done\n"}
    ]);
    let tools = json!([{"type": "function", "name": "functions__shell",
        "description": "Run a command", "parameters": {"type": "object"},
        "strict": false, "defer_loading": true}]);
    assert_eq!(
        build(&request(tools, input.clone())).unwrap(),
        json!({"model": "grok-test", "input": input, "reasoning": null,
            "stream": true, "include": [], "tool_choice": "auto",
            "tools": [{"type": "function", "name": "functions__shell",
                "description": "Run a command", "parameters": {"type": "object"}},
                {"type": "x_search"}]})
    );
}

#[test]
fn absent_and_empty_tools_preserve_basic_request() {
    let mut request = request(json!([]), json!([]));
    let expected = json!({"model": "grok-test", "input": [], "reasoning": null,
        "stream": true, "include": []});
    assert_eq!(build(&request).unwrap(), expected);
    request.tools = None;
    assert_eq!(build(&request).unwrap(), expected);
}

#[test]
fn rejects_unprojected_local_and_unknown_tool_fields() {
    for tools in [
        json!({"type": "function", "name": "shell"}),
        json!([{"type": "namespace", "name": "functions", "tools": []}]),
        json!([{"type": "custom", "name": "apply_patch"}]),
        json!([{"type": "image_generation"}]),
        json!([{"type": "function", "name": "shell", "future_policy": true}]),
        json!([{"type": "function", "name": ""}]),
        json!([{"type": "function", "name": "shell", "parameters": "invalid"}]),
    ] {
        assert!(build(&request(tools, json!([]))).is_err());
    }
}

#[test]
fn hosted_search_preserves_order_and_appends_x_search_once() {
    for (tools, expected) in [
        (
            json!([{"type": "web_search"}]),
            json!([{"type": "web_search"}, {"type": "x_search"}]),
        ),
        (
            json!([{"type": "x_search"}, {"type": "web_search"}]),
            json!([{"type": "x_search"}, {"type": "web_search"}]),
        ),
    ] {
        assert_eq!(
            build(&request(tools, json!([]))).unwrap(),
            json!({"model": "grok-test", "input": [], "reasoning": null,
                "stream": true, "include": [], "tool_choice": "auto", "tools": expected})
        );
    }
    assert!(
        build(&request(
            json!([{"type": "x_search"}, {"type": "x_search"}]),
            json!([])
        ))
        .is_err()
    );
}

#[test]
fn configured_dates_do_not_create_a_tool_plan() {
    let options = GrokXSearchOptions {
        from_date: Some("2026-01-01".into()),
        to_date: Some("2026-01-31".into()),
    };
    let mut request = request(json!([]), json!([]));
    let expected = json!({"model": "grok-test", "input": [], "reasoning": null,
        "stream": true, "include": []});
    assert_eq!(
        build_with_search(&request, Some(&options)).unwrap(),
        expected
    );
    request.tools = None;
    assert_eq!(
        build_with_search(&request, Some(&options)).unwrap(),
        expected
    );
}

#[test]
fn empty_config_and_absent_config_produce_the_same_nonempty_plan() {
    let request = request(json!([{"type": "web_search"}]), json!([]));
    assert_eq!(
        build_with_search(&request, Some(&GrokXSearchOptions::default())).unwrap(),
        build(&request).unwrap()
    );
}

#[test]
fn provider_defaults_are_applied_and_invalid_defaults_cannot_be_overridden() {
    let options = GrokXSearchOptions {
        from_date: Some("2026-01-01".into()),
        to_date: Some("2026-01-31".into()),
    };
    let request = request(json!([{"type": "web_search"}]), json!([]));
    assert_eq!(
        build_with_search(&request, Some(&options)).unwrap()["tools"],
        json!([{"type": "web_search"},
            {"type": "x_search", "from_date": "2026-01-01", "to_date": "2026-01-31"}])
    );
    let invalid = GrokXSearchOptions {
        from_date: Some("2026-02-30".into()),
        ..options
    };
    for tools in [
        json!([]),
        json!([{"type": "x_search", "from_date": "2026-01-01"}]),
    ] {
        let mut request = request.clone();
        request.tools = Some(
            Arc::<serde_json::value::RawValue>::from(
                serde_json::value::to_raw_value(&tools).unwrap(),
            )
            .into(),
        );
        assert!(build_with_search(&request, Some(&invalid)).is_err());
    }
}

#[test]
fn typed_function_output_preserves_image_composition_and_order() {
    let input = json!([{"type": "function_call_output", "call_id": "call_image",
    "output": [
        {"type": "input_text", "text": "Generated image"},
        {"type": "input_image", "image_url": "data:image/png;base64,YQ==", "detail": "original"},
        {"type": "input_text", "text": "Saved to output.png"}
    ]}]);
    let mut request = request(json!([]), input.clone());
    // Internal success is deliberately not a Grok wire field.
    if let ResponseItem::FunctionCallOutput { output, .. } = &mut request.input[0] {
        output.success = Some(true);
    }
    assert_eq!(build(&request).unwrap()["input"], input);
}

#[test]
fn rejects_unprojected_or_unsupported_local_history() {
    for item in [
        json!({"type": "function_call", "name": "shell", "namespace": "functions",
            "arguments": "{}", "call_id": "call_1"}),
        json!({"type": "function_call", "name": "shell", "encrypted_function_args": [],
            "arguments": "{}", "call_id": "call_1"}),
        json!({"type": "function_call_output", "name": "shell", "output": "orphan"}),
        json!({"type": "function_call_output", "call_id": "", "output": "orphan"}),
        json!({"type": "function_call_output", "call_id": "call_1", "namespace": "functions", "output": "done"}),
        json!({"type": "custom_tool_call", "name": "apply_patch", "call_id": "call_1", "input": "patch"}),
        json!({"type": "custom_tool_call_output", "call_id": "call_1", "output": "done"}),
        json!({"type": "web_search_call", "id": "hosted", "status": "completed"}),
    ] {
        assert!(build(&request(json!([]), json!([item]))).is_err());
    }
}

#[test]
fn unsupported_output_content_is_not_silently_dropped() {
    for part in [
        json!({"type": "input_image", "file_id": "file_private"}),
        json!({"type": "input_audio", "audio_url": "data:audio/wav;base64,YQ=="}),
        json!({"type": "encrypted_content", "encrypted_content": "opaque"}),
    ] {
        let input = json!([{"type": "function_call_output", "call_id": "call_1",
            "output": [{"type": "input_text", "text": "before"}, part]}]);
        assert!(build(&request(json!([]), input)).is_err());
    }
}

#[test]
fn completed_hosted_history_preserves_order_ids_and_payload_without_wire_status() {
    let arguments = "{ \"query\": \"launch\", \"n\": 9007199254740993 }\n";
    let input = json!([
        {"type": "web_search_call", "id": "ws_1", "status": "completed",
            "action": {"type": "search", "query": "launch"}},
        {"type": "custom_tool_call", "id": "xs_1", "call_id": "x_1", "status": "completed",
            "name": "x_keyword_search", "input": arguments},
        {"type": "function_call", "id": "fc_1", "call_id": "local_1",
            "name": "functions__shell", "arguments": "{}"}
    ]);
    let request = request(json!([]), input.clone());
    let mut expected = input.clone();
    for index in [0, 1] {
        expected[index].as_object_mut().unwrap().remove("status");
    }
    assert_eq!(build(&request).unwrap()["input"], expected);
    assert_eq!(serde_json::to_value(&request.input).unwrap(), input);
}

#[test]
fn hosted_replay_rejects_incomplete_unknown_and_unidentified_items() {
    let valid_web = json!({"type": "web_search_call", "id": "ws_1", "status": "completed",
        "action": {"type": "search", "query": "launch"}});
    let valid_x = json!({"type": "custom_tool_call", "id": "xs_1", "call_id": "x_1",
        "status": "completed", "name": "x_thread_fetch", "input": "{}"});
    for (original, field, replacement) in [
        (valid_web.clone(), "id", json!(null)),
        (valid_web.clone(), "id", json!("")),
        (valid_web.clone(), "status", json!("in_progress")),
        (valid_web.clone(), "status", json!(null)),
        (
            valid_web.clone(),
            "action",
            json!({"type": "future_action"}),
        ),
        (valid_web, "action", json!(null)),
        (valid_x.clone(), "id", json!(null)),
        (valid_x.clone(), "id", json!("")),
        (valid_x.clone(), "call_id", json!("")),
        (valid_x.clone(), "name", json!("x_unknown_search")),
        (valid_x.clone(), "namespace", json!("functions")),
        (valid_x, "status", json!("failed")),
    ] {
        let mut item = original;
        item[field] = replacement;
        assert!(build(&request(json!([]), json!([item]))).is_err());
    }
}

#[test]
fn empty_structured_outputs_remain_structured() {
    let mut request = request(
        json!([]),
        json!([
            {"type": "function_call_output", "call_id": "call_1", "output": ""}
        ]),
    );
    if let ResponseItem::FunctionCallOutput { output, .. } = &mut request.input[0] {
        *output = FunctionCallOutputPayload::from_content_items(Vec::new());
    }
    assert_eq!(
        build(&request).unwrap()["input"],
        json!([
            {"type": "function_call_output", "call_id": "call_1", "output": []}
        ])
    );
}

#[test]
fn supported_web_replay_preserves_optional_action_fields() {
    for action in [
        json!({"type":"search"}),
        json!({"type":"search", "query":""}),
        json!({"type":"open_page"}),
        json!({"type":"find_in_page", "url":"https://example.com"}),
    ] {
        let item =
            json!({"type":"web_search_call", "id":"web-id", "status":"completed", "action":action});
        let body = build(&request(json!([]), json!([item]))).unwrap();
        assert_eq!(
            body["input"][0],
            json!({"type":"web_search_call", "id":"web-id", "action":action})
        );
    }
}

#[test]
fn plaintext_collaboration_projects_only_the_wire_copy() {
    let task = "Message Type: NEW_TASK\nTask name: /root/child\nSender: /root\nPayload:";
    let result =
        "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/child\nPayload:\nchild result";
    let input = json!([
        {"type": "message", "id": "seed", "role": "user",
            "content": [{"type": "input_text", "text": "seed"}]},
        {"type": "agent_message", "id": "amsg_task", "author": "/root",
            "recipient": "/root/child", "content": [
                {"type": "input_text", "text": task},
                {"type": "input_text", "text": "review"}]},
        {"type": "message", "id": "reply", "role": "assistant",
            "content": [{"type": "output_text", "text": "child result"}]},
        {"type": "agent_message", "id": "amsg_result", "author": "/root/child",
            "recipient": "/root", "content": [{"type": "input_text", "text": result}]}
    ]);
    let request = request(json!([]), input);
    let canonical = serde_json::to_value(&request).unwrap();
    assert_eq!(
        build(&request).unwrap(),
        json!({"model": "grok-test", "reasoning": null, "stream": true,
        "include": [], "input": [
            {"type": "message", "id": "seed", "role": "user",
                "content": [{"type": "input_text", "text": "seed"}]},
            {"type": "message", "id": "amsg_task", "role": "user",
                "content": [{"type": "input_text", "text": format!("{task}\nreview")}]},
            {"type": "message", "id": "reply", "role": "assistant",
                "content": [{"type": "output_text", "text": "child result"}]},
            {"type": "message", "id": "amsg_result", "role": "user",
                "content": [{"type": "input_text", "text": result}]}
        ]})
    );
    assert_eq!(serde_json::to_value(&request).unwrap(), canonical);
}

#[test]
fn encrypted_or_empty_collaboration_is_rejected_without_losing_content() {
    for content in [
        json!([]),
        json!([{"type": "input_text", "text": " \n\t"}]),
        json!([{"type": "encrypted_content", "encrypted_content": "opaque"}]),
        json!([
            {"type": "input_text", "text": "visible prefix"},
            {"type": "encrypted_content", "encrypted_content": "opaque"},
            {"type": "input_text", "text": "visible suffix"}
        ]),
    ] {
        let request = request(
            json!([]),
            json!([
                {"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "seed"}]},
                {"type": "agent_message", "author": "/root", "recipient": "/root/child",
                    "content": content}
            ]),
        );
        let canonical = serde_json::to_value(&request).unwrap();
        let error = build(&request).unwrap_err();
        assert!(matches!(error, ApiError::Stream(message)
            if message == "Grok cannot replay empty or encrypted collaboration history at input[1]"));
        assert_eq!(serde_json::to_value(&request).unwrap(), canonical);
    }
}
