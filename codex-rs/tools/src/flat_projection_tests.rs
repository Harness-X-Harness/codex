use super::*;
use crate::FreeformToolFormat;
use crate::ResponsesApiNamespace;
use pretty_assertions::assert_eq;
use serde_json::json;

fn function(name: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: "Run a task".to_string(),
        strict: false,
        defer_loading: Some(true),
        output_schema: None,
        parameters: serde_json::from_value(
            json!({"type":"object","properties":{"value":{"type":"string"}}}),
        )
        .unwrap(),
    }
}

fn custom(name: &str) -> FreeformTool {
    FreeformTool {
        name: name.to_string(),
        description: "Use exact input".to_string(),
        defer_loading: Some(true),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: /[\\s\\S]+/".to_string(),
        },
    }
}

fn item(value: serde_json::Value) -> ResponseItem {
    serde_json::from_value(value).unwrap()
}

#[test]
fn projection_preserves_function_schema_and_distinguishes_custom_identity() {
    let function = function("run");
    let specs = vec![
        ToolSpec::Function(function.clone()),
        ToolSpec::Freeform(custom("run")),
    ];
    let before = specs.clone();
    let (projected, routes) = project_flat_function_tools(&specs).unwrap();
    let function_name = flat_wire_name("function", &ToolName::plain("run"));
    let custom_name = flat_wire_name("custom", &ToolName::plain("run"));
    let mut expected = function;
    expected.name = function_name.clone();
    expected.defer_loading = None;
    expected.description = "Call this function directly to invoke `run`. Do not invoke it through another tool.\n\nRun a task".to_string();
    let expected_custom = ResponsesApiTool {
        name: custom_name.clone(),
        description: "Call this function directly to invoke `run`. Do not invoke it through another tool.\n\nUse exact input\n\nstart: /[\\s\\S]+/".to_string(),
        strict: true,
        defer_loading: None,
        parameters: serde_json::from_value(json!({
            "type": "object",
            "properties": {"input": {"type": "string", "description": "Freeform input passed unchanged to the tool."}},
            "required": ["input"],
            "additionalProperties": false,
        })).unwrap(),
        output_schema: None,
    };
    assert_eq!(
        projected,
        vec![
            ToolSpec::Function(expected),
            ToolSpec::Function(expected_custom)
        ]
    );
    assert_eq!(
        routes.resolve(&function_name),
        Some(&WireToolRoute::Function(ToolName::plain("run")))
    );
    assert_eq!(
        routes.resolve(&custom_name),
        Some(&WireToolRoute::Custom {
            tool_name: ToolName::plain("run"),
            input_key: "input".to_string()
        })
    );
    assert_eq!(specs, before);
}

#[test]
fn namespace_normalization_is_stable_and_duplicates_fail_closed() {
    let top = vec![ToolSpec::Function(function("run"))];
    for namespace in ["", "functions"] {
        let namespaced = ToolSpec::Namespace(ResponsesApiNamespace {
            name: namespace.to_string(),
            description: "namespace".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(function("run"))],
        });
        assert_eq!(
            project_flat_function_tools(&top).unwrap().0,
            project_flat_function_tools(&[namespaced.clone()])
                .unwrap()
                .0
        );
        assert!(project_flat_function_tools(&[top[0].clone(), namespaced]).is_err());
    }
    assert_ne!(
        flat_wire_name("function", &ToolName::namespaced("a", "b_c")),
        flat_wire_name("function", &ToolName::namespaced("a_b", "c"))
    );
}

#[test]
fn long_unicode_names_are_bounded_and_collision_checked() {
    let name = ToolName::namespaced("连接".repeat(80), "工具".repeat(80));
    let wire = flat_wire_name("function", &name);
    assert!(wire.len() <= 128);
    assert!(
        wire.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let mut routes = FlatToolRoutes::default();
    routes
        .by_wire_name
        .insert(wire, WireToolRoute::Function(ToolName::plain("different")));
    assert!(routes.remember(WireToolRoute::Function(name)).is_err());
}

#[test]
fn completed_function_calls_roundtrip_without_reencoding_arguments() {
    let specs = vec![ToolSpec::Namespace(ResponsesApiNamespace {
        name: "mcp__server".to_string(),
        description: String::new(),
        tools: vec![ResponsesApiNamespaceTool::Function(function("run"))],
    })];
    let (_, routes) = project_flat_function_tools(&specs).unwrap();
    let canonical = item(
        json!({"type":"function_call","id":"fc_test","name":"run","namespace":"mcp__server","arguments":" {\"value\": \"é\\n\"} \n","call_id":"call_1"}),
    );
    let before = canonical.clone();
    let projected = routes.project_history(&[canonical.clone()]).unwrap();
    let mut expected = canonical.clone();
    if let ResponseItem::FunctionCall {
        name, namespace, ..
    } = &mut expected
    {
        *name = flat_wire_name("function", &ToolName::namespaced("mcp__server", "run"));
        *namespace = None;
    }
    assert_eq!(projected, vec![expected]);
    assert_eq!(
        routes.restore_response_item(projected[0].clone()).unwrap(),
        canonical
    );
    assert_eq!(canonical, before);
}

#[test]
fn custom_history_and_structured_output_project_without_mutation() {
    let (_, routes) =
        project_flat_function_tools(&[ToolSpec::Freeform(custom("apply_patch"))]).unwrap();
    let input = "*** Begin Patch\n雪\\\"\n*** End Patch\n";
    let history = vec![
        item(
            json!({"type":"custom_tool_call","id":"ctc_1","status":"completed","call_id":"call_1","name":"apply_patch","input":input}),
        ),
        item(
            json!({"type":"custom_tool_call_output","id":"cto_1","call_id":"call_1","name":"apply_patch","output":[{"type":"input_text","text":"done"}]}),
        ),
    ];
    let before = history.clone();
    let wire = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    let arguments = serde_json::to_string(&json!({"patch":input})).unwrap();
    let expected = vec![
        item(
            json!({"type":"function_call","id":"ctc_1","call_id":"call_1","name":wire,"arguments":arguments}),
        ),
        item(
            json!({"type":"function_call_output","id":"cto_1","call_id":"call_1","name":wire,"output":[{"type":"input_text","text":"done"}]}),
        ),
    ];
    let projected = routes.project_history(&history).unwrap();
    assert_eq!(projected, expected);
    let mut restored = before[0].clone();
    if let ResponseItem::CustomToolCall { status, .. } = &mut restored {
        *status = None;
    }
    assert_eq!(
        routes.restore_response_item(projected[0].clone()).unwrap(),
        restored
    );
    assert_eq!(history, before);
}

#[test]
fn old_history_can_replay_but_cannot_authorize_incoming_calls() {
    let routes = FlatToolRoutes::default();
    let history = vec![
        item(
            json!({"type":"custom_tool_call","call_id":"old","namespace":"remote","name":"apply_patch","input":"exact\n"}),
        ),
        item(json!({"type":"custom_tool_call_output","call_id":"old","output":"ok"})),
    ];
    let wire = flat_wire_name("custom", &ToolName::namespaced("remote", "apply_patch"));
    let projected = routes.project_history(&history).unwrap();
    assert_eq!(
        projected,
        vec![
            item(
                json!({"type":"function_call","call_id":"old","name":wire,"arguments":"{\"input\":\"exact\\n\"}"})
            ),
            item(json!({"type":"function_call_output","call_id":"old","output":"ok"})),
        ]
    );
    assert!(routes.restore_response_item(projected[0].clone()).is_err());
    assert!(routes.project_history(&history[1..]).is_err());
}

#[test]
fn custom_wrappers_reject_ambiguous_or_malformed_inputs() {
    for arguments in [
        "null",
        "[]",
        "{}",
        "{\"input\":3}",
        "{\"input\":\"a\",\"extra\":0}",
        "{\"input\":\"a\",\"input\":\"b\"}",
        "{\"patch\":\"a\"}",
        "{\"input\":\"a\"} trailing",
    ] {
        assert!(
            decode_custom_input("wire", arguments, "input").is_err(),
            "{arguments}"
        );
    }
    assert_eq!(
        decode_custom_input("wire", "{\"input\":\"  雪\\n\\\\\\\"  \"}", "input").unwrap(),
        "  雪\n\\\"  "
    );
}

#[test]
fn hosted_and_search_tools_are_explicitly_unsupported() {
    let search = ToolSpec::ToolSearch {
        execution: "client".to_string(),
        description: String::new(),
        parameters: function("unused").parameters,
    };
    assert!(project_flat_function_tools(&[search]).is_err());
    let hosted = ToolSpec::WebSearch {
        external_web_access: None,
        indexed_web_access: None,
        filters: None,
        user_location: None,
        search_context_size: None,
        search_content_types: None,
    };
    assert!(project_flat_function_tools(&[hosted]).is_err());
    assert!(
        FlatToolRoutes::default()
            .project_history(&[item(json!({"type":"web_search_call"}))])
            .is_err()
    );
}

#[test]
fn paired_function_output_uses_call_namespace_when_output_omits_it() {
    let history = vec![
        item(
            json!({"type":"function_call","call_id":"call_1","name":"run","namespace":"server","arguments":" { } "}),
        ),
        item(
            json!({"type":"function_call_output","id":"fco_1","call_id":"call_1","name":"run","output":"result"}),
        ),
    ];
    let wire = flat_wire_name("function", &ToolName::namespaced("server", "run"));
    assert_eq!(
        FlatToolRoutes::default().project_history(&history).unwrap(),
        vec![
            item(
                json!({"type":"function_call","call_id":"call_1","name":wire,"arguments":" { } "})
            ),
            item(
                json!({"type":"function_call_output","id":"fco_1","call_id":"call_1","name":wire,"output":"result"})
            ),
        ]
    );
}

#[test]
fn ingress_rejects_canonical_fallback_and_nonflat_calls() {
    let (_, routes) =
        project_flat_function_tools(&[ToolSpec::Freeform(custom("apply_patch"))]).unwrap();
    let wire = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    for value in [
        json!({"type":"function_call","call_id":"call_1","name":"apply_patch","arguments":"{\"patch\":\"text\"}"}),
        json!({"type":"function_call","call_id":"call_1","name":wire,"namespace":"remote","arguments":"{\"patch\":\"text\"}"}),
        json!({"type":"function_call","call_id":"call_1","name":wire,"arguments":"{\"patch\":1}"}),
        json!({"type":"custom_tool_call","call_id":"call_1","name":"apply_patch","input":"text"}),
    ] {
        assert!(routes.restore_response_item(item(value)).is_err());
    }
}

#[test]
fn outgoing_collaboration_logging_markers_are_removed_only_from_the_copy() {
    for name in ["spawn_agent", "send_message", "followup_task"] {
        let canonical = item(json!({
            "type": "function_call", "id": "fc_marker", "call_id": "call_marker",
            "namespace": "collaboration", "name": name,
            "arguments": " {\"message\": \"unchanged\\n\"} ",
            "encrypted_function_args": [],
        }));
        let before = canonical.clone();
        let wire = flat_wire_name("function", &ToolName::namespaced("collaboration", name));
        let expected = item(json!({
            "type": "function_call", "id": "fc_marker", "call_id": "call_marker",
            "name": wire, "arguments": " {\"message\": \"unchanged\\n\"} ",
        }));
        assert_eq!(
            FlatToolRoutes::default()
                .project_history(&[canonical.clone()])
                .unwrap(),
            vec![expected]
        );
        assert_eq!(canonical, before);
    }
}

#[test]
fn outgoing_actual_encryption_and_unrecognized_empty_markers_fail_closed() {
    for (namespace, name, envelope) in [
        (Some("collaboration"), "spawn_agent", json!(["encrypted"])),
        (Some("collaboration"), "send_message", json!(["encrypted"])),
        (Some("collaboration"), "followup_task", json!(["encrypted"])),
        (Some("collaboration"), "wait", json!([])),
        (Some("other"), "send_message", json!([])),
        (None, "spawn_agent", json!([])),
        (Some("functions"), "followup_task", json!([])),
        (Some("mcp__server"), "run", json!(["encrypted"])),
    ] {
        let canonical = item(json!({
            "type": "function_call", "call_id": "call_1", "namespace": namespace,
            "name": name, "arguments": "{}", "encrypted_function_args": envelope,
        }));
        let before = canonical.clone();
        assert!(
            FlatToolRoutes::default()
                .project_history(&[canonical.clone()])
                .is_err()
        );
        assert_eq!(canonical, before);
    }
}

#[test]
fn ingress_rejects_every_encrypted_envelope_before_restoring_any_route() {
    let (_, routes) = project_flat_function_tools(&[
        ToolSpec::Function(function("run")),
        ToolSpec::Freeform(custom("apply_patch")),
    ])
    .unwrap();
    for (kind, name, arguments) in [
        ("function", "run", " {\"value\": \"exact\\n\"} "),
        ("custom", "apply_patch", "{\"patch\":\"exact\\n\"}"),
    ] {
        let wire = flat_wire_name(kind, &ToolName::plain(name));
        for envelope in [json!([]), json!(["encrypted"])] {
            let incoming = item(json!({
                "type": "function_call", "call_id": "call_1", "name": wire,
                "arguments": arguments, "encrypted_function_args": envelope,
            }));
            assert_eq!(
                routes.restore_response_item(incoming),
                Err("flat function call cannot contain encrypted arguments".to_string())
            );
        }
    }
}
