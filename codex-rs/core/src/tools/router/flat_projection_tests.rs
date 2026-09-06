use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolRegistry;
use crate::tools::router::ToolRouter;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ToolMode;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

fn plain_function(name: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: format!("Call {name}."),
        strict: true,
        parameters: codex_extension_api::parse_tool_input_schema(&json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
            "additionalProperties": false,
        }))
        .expect("test schema should parse"),
        output_schema: None,
        defer_loading: None,
    }
}

fn apply_patch_freeform() -> FreeformTool {
    FreeformTool {
        name: "apply_patch".to_string(),
        description: "Apply a patch.".to_string(),
        defer_loading: None,
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: /.+/".to_string(),
        },
    }
}

fn function(name: &str) -> ResponsesApiNamespaceTool {
    ResponsesApiNamespaceTool::Function(ResponsesApiTool {
        name: name.to_string(),
        description: format!("Call {name}."),
        strict: true,
        parameters: codex_extension_api::parse_tool_input_schema(&json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
            "additionalProperties": false,
        }))
        .expect("test schema should parse"),
        output_schema: None,
        defer_loading: None,
    })
}

#[test]
fn flat_projection_disambiguates_retained_indirect_guidance() -> anyhow::Result<()> {
    let mut imagegen = match function("imagegen") {
        ResponsesApiNamespaceTool::Function(tool) => tool,
        ResponsesApiNamespaceTool::Custom(_) => unreachable!("fixture is a function"),
    };
    imagegen.description =
        "Generate or edit an image. In code-mode, invoke this tool through the exec wrapper."
            .to_string();
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![ToolSpec::Namespace(ResponsesApiNamespace {
            name: "image_gen".to_string(),
            description: "Image tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(imagegen)],
        })],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let description = match &router.model_visible_specs()[0] {
        ToolSpec::Function(tool) => tool.description.clone(),
        spec => panic!("expected projected function, got {spec:?}"),
    };

    assert!(description.contains("canonical `image_gen.imagegen` tool"));
    assert!(description.contains("Call this function itself."));
    assert!(description.ends_with(
        "Generate or edit an image. In code-mode, invoke this tool through the exec wrapper."
    ));
    Ok(())
}

#[test]
fn flat_projection_round_trips_same_local_names_across_namespaces() -> anyhow::Result<()> {
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![
            ToolSpec::Namespace(ResponsesApiNamespace {
                name: "mcp__calendar".to_string(),
                description: "Calendar tools.".to_string(),
                tools: vec![function("create_event")],
            }),
            ToolSpec::Namespace(ResponsesApiNamespace {
                name: "mcp__tasks".to_string(),
                description: "Task tools.".to_string(),
                tools: vec![function("create_event")],
            }),
            ToolSpec::Namespace(ResponsesApiNamespace {
                name: "collaboration".to_string(),
                description: "Agent tools.".to_string(),
                tools: vec![function("spawn_agent"), function("send_message")],
            }),
        ],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let wire_names = router
        .model_visible_specs()
        .iter()
        .map(|spec| match spec {
            ToolSpec::Function(tool) => tool.name.clone(),
            spec => panic!("expected projected function, got {spec:?}"),
        })
        .collect::<Vec<_>>();
    let descriptions = router
        .model_visible_specs()
        .iter()
        .map(|spec| match spec {
            ToolSpec::Function(tool) => tool.description.clone(),
            spec => panic!("expected projected function, got {spec:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(wire_names.len(), 4);
    assert_ne!(wire_names[0], wire_names[1]);
    assert!(descriptions[0].contains("canonical `mcp__calendar.create_event` tool"));
    assert!(descriptions[0].contains("Call this function itself."));
    assert!(descriptions[0].ends_with("Call create_event."));

    let mut items = wire_names
        .iter()
        .enumerate()
        .map(|(index, name)| ResponseItem::FunctionCall {
            id: None,
            name: name.clone(),
            namespace: None,
            arguments: json!({"value": format!("argument-{index}")}).to_string(),
            encrypted_function_args: None,
            call_id: format!("call-{index}"),
            internal_chat_message_metadata_passthrough: None,
        })
        .collect::<Vec<_>>();
    let wire_items = items.clone();
    for item in &mut items {
        router.restore_tool_call(item)?;
    }
    assert_eq!(router.project_tool_wire(items.clone()), wire_items);
    let standalone_output = FunctionCallOutputPayload::from_text("created".to_string());
    assert_eq!(
        router.project_tool_wire(vec![ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: Some("create_event".to_string()),
            namespace: Some("mcp__calendar".to_string()),
            output: standalone_output.clone(),
            internal_chat_message_metadata_passthrough: None,
        }]),
        vec![ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: Some(wire_names[0].clone()),
            namespace: None,
            output: standalone_output,
            internal_chat_message_metadata_passthrough: None,
        }]
    );
    let calls = items
        .into_iter()
        .map(|item| {
            ToolRouter::build_tool_call(item)
                .expect("restored call should parse")
                .expect("restored item should be a tool call")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        calls
            .iter()
            .map(|call| call.tool_name.clone())
            .collect::<Vec<_>>(),
        vec![
            ToolName::namespaced("mcp__calendar", "create_event"),
            ToolName::namespaced("mcp__tasks", "create_event"),
            ToolName::namespaced("collaboration", "spawn_agent"),
            ToolName::namespaced("collaboration", "send_message"),
        ]
    );
    assert_eq!(
        calls
            .iter()
            .map(|call| call.payload.clone())
            .collect::<Vec<_>>(),
        vec![
            ToolPayload::Function {
                arguments: json!({"value": "argument-0"}).to_string(),
            },
            ToolPayload::Function {
                arguments: json!({"value": "argument-1"}).to_string(),
            },
            ToolPayload::Function {
                arguments: json!({"value": "argument-2"}).to_string(),
            },
            ToolPayload::Function {
                arguments: json!({"value": "argument-3"}).to_string(),
            },
        ]
    );
    for call in &calls[..2] {
        assert_eq!(call.encrypted_function_args, None);
        assert_eq!(call.direct_source(), ToolCallSource::Direct);
    }
    for call in &calls[2..] {
        assert_eq!(call.encrypted_function_args, Some(Vec::new()));
        assert_eq!(call.direct_source(), ToolCallSource::DirectPlaintextMessage);
    }
    Ok(())
}

#[test]
fn flat_projection_replays_custom_call_with_matching_output() -> anyhow::Result<()> {
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![ToolSpec::Namespace(ResponsesApiNamespace {
            name: "editor".to_string(),
            description: "Editing tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Custom(FreeformTool {
                name: "apply_patch".to_string(),
                description: "Apply a patch.".to_string(),
                defer_loading: None,
                format: FreeformToolFormat {
                    r#type: "grammar".to_string(),
                    syntax: "lark".to_string(),
                    definition: "start: /.+/".to_string(),
                },
            })],
        })],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let model_visible_specs = router.model_visible_specs();
    let declared_name = match &model_visible_specs[0] {
        ToolSpec::Function(tool) => tool.name.as_str(),
        spec => panic!("expected projected function, got {spec:?}"),
    };
    let output = FunctionCallOutputPayload::from_text("done".to_string());
    let projected = router.project_tool_wire(vec![
        ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-custom".to_string(),
            name: "apply_patch".to_string(),
            namespace: Some("editor".to_string()),
            input: "patch".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CustomToolCallOutput {
            id: None,
            call_id: "call-custom".to_string(),
            name: Some("apply_patch".to_string()),
            output: output.clone(),
            internal_chat_message_metadata_passthrough: None,
        },
    ]);

    assert!(matches!(
        &projected[0],
        ResponseItem::FunctionCall { name, call_id, .. }
            if name == declared_name && call_id == "call-custom"
    ));
    assert!(matches!(
        &projected[1],
        ResponseItem::FunctionCallOutput {
            call_id: Some(call_id),
            output: projected_output,
            ..
        } if call_id == "call-custom" && projected_output == &output
    ));
    Ok(())
}

#[test]
fn flat_projection_declares_plain_apply_patch_as_function_with_patch() -> anyhow::Result<()> {
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![ToolSpec::Freeform(apply_patch_freeform())],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let tool = match &router.model_visible_specs()[0] {
        ToolSpec::Function(tool) => tool,
        spec => panic!("expected projected function, got {spec:?}"),
    };
    let properties = tool
        .parameters
        .properties
        .as_ref()
        .expect("projected apply_patch should declare properties");
    assert!(tool.description.contains("canonical `apply_patch` tool"));
    assert!(tool.description.contains("Call this function itself."));
    assert!(properties.contains_key("patch"));
    assert_eq!(tool.parameters.required, Some(vec!["patch".to_string()]));
    Ok(())
}

#[test]
fn flat_projection_replays_plain_apply_patch_custom_call() -> anyhow::Result<()> {
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![ToolSpec::Freeform(apply_patch_freeform())],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let declared_name = match &router.model_visible_specs()[0] {
        ToolSpec::Function(tool) => tool.name.clone(),
        spec => panic!("expected projected function, got {spec:?}"),
    };
    let output = FunctionCallOutputPayload::from_text("done".to_string());
    let projected = router.project_tool_wire(vec![
        ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-plain-apply-patch".to_string(),
            name: "apply_patch".to_string(),
            namespace: None,
            input: "*** Begin Patch".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CustomToolCallOutput {
            id: None,
            call_id: "call-plain-apply-patch".to_string(),
            name: Some("apply_patch".to_string()),
            output: output.clone(),
            internal_chat_message_metadata_passthrough: None,
        },
    ]);

    assert!(matches!(
        &projected[0],
        ResponseItem::FunctionCall { name, call_id, arguments, .. }
            if name == &declared_name
                && call_id == "call-plain-apply-patch"
                && arguments.contains("\"patch\"")
    ));
    assert!(matches!(
        &projected[1],
        ResponseItem::FunctionCallOutput {
            call_id: Some(call_id),
            output: projected_output,
            ..
        } if call_id == "call-plain-apply-patch" && projected_output == &output
    ));
    Ok(())
}

#[test]
fn flat_projection_keeps_apply_patch_and_unified_exec_names_distinct() -> anyhow::Result<()> {
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![
            ToolSpec::Function(plain_function("exec_command")),
            ToolSpec::Function(plain_function("write_stdin")),
            ToolSpec::Freeform(apply_patch_freeform()),
        ],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)?;
    let names = router
        .model_visible_specs()
        .iter()
        .map(|spec| match spec {
            ToolSpec::Function(tool) => tool.name.clone(),
            spec => panic!("expected projected function, got {spec:?}"),
        })
        .collect::<Vec<_>>();
    let descriptions = router
        .model_visible_specs()
        .iter()
        .map(|spec| match spec {
            ToolSpec::Function(tool) => tool.description.clone(),
            spec => panic!("expected projected function, got {spec:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 3);
    assert_ne!(names[0], names[1]);
    assert_ne!(names[0], names[2]);
    assert_ne!(names[1], names[2]);
    assert!(descriptions[0].contains("canonical `exec_command` tool"));
    assert!(descriptions[1].contains("canonical `write_stdin` tool"));
    assert!(descriptions[2].contains("canonical `apply_patch` tool"));
    Ok(())
}

#[test]
fn flat_projection_marks_only_plaintext_collaboration_calls() -> anyhow::Result<()> {
    let child_management_tools = [
        ToolName::namespaced("collaboration", "spawn_agent"),
        ToolName::namespaced("collaboration", "wait_agent"),
    ];
    let router = ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![ToolSpec::Namespace(ResponsesApiNamespace {
            name: "collaboration".to_string(),
            description: "Agent tools.".to_string(),
            tools: vec![function("spawn_agent"), function("wait_agent")],
        })],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &child_management_tools,
        true,
    )
    .map_err(anyhow::Error::msg)?;
    assert!(router.can_manage_children());
    let wire_names = router
        .model_visible_specs()
        .iter()
        .map(|spec| match spec {
            ToolSpec::Function(tool) => tool.name.clone(),
            spec => panic!("expected projected function, got {spec:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(wire_names.len(), 2);

    let mut items = wire_names
        .iter()
        .enumerate()
        .map(|(index, name)| ResponseItem::FunctionCall {
            id: None,
            name: name.clone(),
            namespace: None,
            arguments: format!(r#"{{"value":"{index}"}}"#),
            encrypted_function_args: None,
            call_id: format!("call-{index}"),
            internal_chat_message_metadata_passthrough: None,
        })
        .collect::<Vec<_>>();
    for item in &mut items {
        router.restore_tool_call(item)?;
    }
    let calls = items
        .into_iter()
        .map(|item| {
            ToolRouter::build_tool_call(item)
                .expect("restored call should parse")
                .expect("restored item should be a tool call")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls[0].tool_name,
        ToolName::namespaced("collaboration", "spawn_agent")
    );
    assert_eq!(calls[0].encrypted_function_args, Some(Vec::new()));
    assert_eq!(
        calls[0].direct_source(),
        ToolCallSource::DirectPlaintextMessage
    );
    assert_eq!(
        calls[1].tool_name,
        ToolName::namespaced("collaboration", "wait_agent")
    );
    assert_eq!(calls[1].encrypted_function_args, None);
    assert_eq!(calls[1].direct_source(), ToolCallSource::Direct);
    Ok(())
}
