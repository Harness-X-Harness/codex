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
                "description": "Run a command", "parameters": {"type": "object"}}]})
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
fn rejects_unprojected_hosted_and_unknown_tool_fields() {
    for tools in [
        json!({"type": "function", "name": "shell"}),
        json!([{"type": "namespace", "name": "functions", "tools": []}]),
        json!([{"type": "custom", "name": "apply_patch"}]),
        json!([{"type": "web_search"}]),
        json!([{"type": "x_search"}]),
        json!([{"type": "function", "name": "shell", "future_policy": true}]),
        json!([{"type": "function", "name": ""}]),
        json!([{"type": "function", "name": "shell", "parameters": "invalid"}]),
    ] {
        assert!(build(&request(tools, json!([]))).is_err());
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
