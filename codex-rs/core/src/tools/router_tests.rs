use std::collections::BTreeMap;
use std::sync::Arc;

use crate::config::Config;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::McpHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::RegisteredTool;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::ToolRegistry;
use crate::tools::spec_plan::append_source_tools;
use crate::tools::spec_plan::build_core_tool_registry;
use crate::tools::spec_plan::extension_tool_executors;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall as ExtensionToolCall;
use codex_extension_api::ToolExecutor;
use codex_protocol::DEFAULT_FUNCTION_NAMESPACE;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use codex_protocol::dynamic_tools::DynamicToolNamespaceSpec;
use codex_protocol::dynamic_tools::DynamicToolNamespaceTool;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ToolMode;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::default_namespace_description;
use core_test_support::responses::strip_response_item_ids_from_json;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::ToolCall;
use super::ToolCallSource;
use super::ToolRouter;
use super::tool_log_payload;

struct ExtensionEchoContributor;

#[test]
fn tool_log_payload_redacts_plaintext_multi_agent_messages() {
    let payload = ToolPayload::Function {
        arguments: json!({"target": "/root/worker", "message": "secret message"}).to_string(),
    };
    assert_eq!(
        tool_log_payload(&payload, &ToolCallSource::DirectPlaintextMessage),
        "[plaintext arguments]"
    );
    assert_eq!(
        tool_log_payload(&payload, &ToolCallSource::Direct),
        payload.log_payload()
    );
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
    assert_eq!(router.project_model_input(items.clone()), wire_items);
    let standalone_output = FunctionCallOutputPayload::from_text("created".to_string());
    assert_eq!(
        router.project_model_input(vec![ResponseItem::FunctionCallOutput {
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
    let declared_name = match &router.model_visible_specs()[0] {
        ToolSpec::Function(tool) => tool.name.as_str(),
        spec => panic!("expected projected function, got {spec:?}"),
    };
    let output = FunctionCallOutputPayload::from_text("done".to_string());
    let projected = router.project_model_input(vec![
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

impl codex_extension_api::ToolContributor for ExtensionEchoContributor {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        _thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ExtensionToolCall<'call>>>> {
        vec![Arc::new(ExtensionEchoExecutor)]
    }
}

struct ExtensionEchoExecutor;

impl<'call> ToolExecutor<ExtensionToolCall<'call>> for ExtensionEchoExecutor {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced("extension/", "echo")
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: "extension/".to_string(),
            description: default_namespace_description("extension/"),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: "echo".to_string(),
                description: "Echoes arguments through an extension tool.".to_string(),
                strict: true,
                parameters: codex_extension_api::parse_tool_input_schema(&json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" },
                    },
                    "required": ["message"],
                    "additionalProperties": false,
                }))
                .expect("extension schema should parse"),
                output_schema: None,
                defer_loading: None,
            })],
        })
    }

    fn handle<'a>(&'a self, call: ExtensionToolCall<'call>) -> codex_tools::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

impl ExtensionEchoExecutor {
    async fn handle_call(
        &self,
        call: ExtensionToolCall<'_>,
    ) -> Result<Box<dyn codex_tools::ToolOutput>, codex_tools::FunctionCallError> {
        let arguments: serde_json::Value =
            serde_json::from_str(call.function_arguments()?).expect("test arguments should parse");
        Ok(Box::new(codex_tools::JsonToolOutput::new(json!({
            "arguments": arguments,
            "callId": call.call_id,
            "conversationHistory": call.conversation_history.items(),
            "ok": true,
        }))) as Box<dyn codex_tools::ToolOutput>)
    }
}

fn extension_tool_test_registry() -> Arc<ExtensionRegistry<Config>> {
    let mut builder = ExtensionRegistryBuilder::new();
    builder.tool_contributor(Arc::new(ExtensionEchoContributor));
    Arc::new(builder.build())
}

fn test_tool_router(
    step_context: &StepContext,
    mcp_tools: Vec<RegisteredTool>,
    extension_tool_executors: impl IntoIterator<
        Item = Arc<dyn for<'call> ToolExecutor<ExtensionToolCall<'call>>>,
    >,
    dynamic_tools: &[DynamicToolSpec],
) -> ToolRouter {
    let mut registry = build_core_tool_registry(
        step_context.turn.as_ref(),
        step_context.turn.model_info(),
        &step_context.environments,
        step_context.mcp.as_ref(),
        /*tool_suggest_candidates*/ None,
        /*wait_for_environment_tool_config*/ None,
    );
    let hosted_specs = append_source_tools(
        step_context.turn.as_ref(),
        step_context.turn.model_info(),
        &mut registry,
        mcp_tools,
        extension_tool_executors,
        dynamic_tools,
    );
    ToolRouter::from_registry(
        step_context.turn.as_ref(),
        step_context.turn.model_info(),
        registry,
        hosted_specs,
        &Default::default(),
    )
}

#[tokio::test]
async fn parallel_support_does_not_match_namespaced_local_tool_names() -> anyhow::Result<()> {
    let (_, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let router = test_tool_router(
        step_context.as_ref(),
        Vec::new(),
        Vec::new(),
        &turn.dynamic_tools,
    );

    let parallel_tool_name = ["exec_command"]
        .into_iter()
        .find(|name| {
            router.tool_supports_parallel(&ToolCall {
                tool_name: ToolName::plain(*name),
                call_id: "call-parallel-tool".to_string(),
                payload: ToolPayload::Function {
                    arguments: "{}".to_string(),
                },
                encrypted_function_args: None,
            })
        })
        .expect("test session should expose a parallel shell-like tool");

    assert_eq!(
        router
            .tool_runtime(&ToolCall {
                tool_name: ToolName::plain(parallel_tool_name),
                call_id: "call-local-tool".to_string(),
                payload: ToolPayload::Function {
                    arguments: "{}".to_string(),
                },
                encrypted_function_args: None,
            })
            .map(|runtime| runtime.tool_name()),
        Some(ToolName::plain(parallel_tool_name))
    );

    assert!(!router.tool_supports_parallel(&ToolCall {
        tool_name: ToolName::namespaced("mcp__server__", parallel_tool_name),
        call_id: "call-namespaced-tool".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    }));

    Ok(())
}

#[tokio::test]
async fn build_tool_call_uses_namespace_for_registry_name() -> anyhow::Result<()> {
    let tool_name = "create_event".to_string();

    let call = ToolRouter::build_tool_call(ResponseItem::FunctionCall {
        id: None,
        name: tool_name.clone(),
        namespace: Some("mcp__codex_apps__calendar".to_string()),
        arguments: "{}".to_string(),
        encrypted_function_args: Some(Vec::new()),
        call_id: "call-namespace".to_string(),
        internal_chat_message_metadata_passthrough: None,
    })?
    .expect("function_call should produce a tool call");

    assert_eq!(
        call.tool_name,
        ToolName::namespaced("mcp__codex_apps__calendar", tool_name)
    );
    assert_eq!(call.call_id, "call-namespace");
    assert_eq!(call.encrypted_function_args, Some(Vec::new()));
    assert_eq!(call.direct_source(), ToolCallSource::Direct);
    match call.payload {
        ToolPayload::Function { arguments } => {
            assert_eq!(arguments, "{}");
        }
        other => panic!("expected function payload, got {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn build_custom_tool_call_uses_namespace_for_registry_name() -> anyhow::Result<()> {
    let tool_name = "exec".to_string();

    let call = ToolRouter::build_tool_call(ResponseItem::CustomToolCall {
        id: None,
        status: None,
        call_id: "call-namespace".to_string(),
        name: tool_name.clone(),
        namespace: Some("mcp__python".to_string()),
        input: "print('hello')".to_string(),
        internal_chat_message_metadata_passthrough: None,
    })?
    .expect("custom_tool_call should produce a tool call");

    assert_eq!(
        call,
        ToolCall {
            tool_name: ToolName::namespaced("mcp__python", tool_name),
            call_id: "call-namespace".to_string(),
            payload: ToolPayload::Custom {
                input: "print('hello')".to_string(),
            },
            encrypted_function_args: None,
        }
    );

    Ok(())
}

#[test]
fn build_tool_call_normalizes_default_function_and_custom_namespaces() -> anyhow::Result<()> {
    for namespace in [None, Some(""), Some(DEFAULT_FUNCTION_NAMESPACE)] {
        let function_call = ToolRouter::build_tool_call(ResponseItem::FunctionCall {
            id: None,
            name: "lookup".to_string(),
            namespace: namespace.map(str::to_string),
            arguments: "{}".to_string(),
            encrypted_function_args: None,
            call_id: "call-function".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })?
        .expect("function_call should produce a tool call");
        let custom_call = ToolRouter::build_tool_call(ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-custom".to_string(),
            name: "apply_patch".to_string(),
            namespace: namespace.map(str::to_string),
            input: "patch".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })?
        .expect("custom_tool_call should produce a tool call");

        assert_eq!(
            [function_call.tool_name, custom_call.tool_name],
            [
                ToolName::namespaced(DEFAULT_FUNCTION_NAMESPACE, "lookup"),
                ToolName::namespaced(DEFAULT_FUNCTION_NAMESPACE, "apply_patch"),
            ]
        );
    }

    Ok(())
}

#[tokio::test]
async fn mcp_parallel_support_uses_handler_data() -> anyhow::Result<()> {
    let (_, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let router = test_tool_router(
        step_context.as_ref(),
        vec![
            mcp_runtime(mcp_tool_info(
                "echo",
                /*supports_parallel_tool_calls*/ true,
                "mcp__echo__",
                "query_with_delay",
            )),
            RegisteredTool {
                exposure: ToolExposure::DirectModelOnly,
                ..mcp_runtime(mcp_tool_info(
                    "hello_echo",
                    /*supports_parallel_tool_calls*/ false,
                    "mcp__hello_echo__",
                    "query_with_delay",
                ))
            },
            RegisteredTool {
                exposure: ToolExposure::Hidden,
                ..mcp_runtime(mcp_tool_info(
                    "hidden_echo",
                    /*supports_parallel_tool_calls*/ true,
                    "mcp__hidden_echo__",
                    "query_with_delay",
                ))
            },
            RegisteredTool {
                exposure: ToolExposure::CodeModeOnly,
                ..mcp_runtime(mcp_tool_info(
                    "nested_echo",
                    /*supports_parallel_tool_calls*/ true,
                    "mcp__nested_echo__",
                    "query_with_delay",
                ))
            },
        ],
        Vec::new(),
        &turn.dynamic_tools,
    );

    let call = ToolCall {
        tool_name: ToolName::namespaced("mcp__echo__", "query_with_delay"),
        call_id: "call-handler".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    };
    assert!(router.tool_supports_parallel(&call));
    assert_eq!(
        router
            .tool_runtime(&call)
            .map(|runtime| runtime.tool_name()),
        Some(call.tool_name.clone())
    );

    let different_server_call = ToolCall {
        tool_name: ToolName::namespaced("mcp__hello_echo__", "query_with_delay"),
        call_id: "call-other-server".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    };
    assert!(!router.tool_supports_parallel(&different_server_call));
    assert_eq!(
        router
            .tool_runtime(&different_server_call)
            .map(|runtime| runtime.tool_name()),
        Some(different_server_call.tool_name.clone())
    );

    let hidden_call = ToolCall {
        tool_name: ToolName::namespaced("mcp__hidden_echo__", "query_with_delay"),
        call_id: "call-hidden".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    };
    assert!(!router.tool_supports_parallel(&hidden_call));
    assert!(router.tool_runtime(&hidden_call).is_some());

    let nested_only_call = ToolCall {
        tool_name: ToolName::namespaced("mcp__nested_echo__", "query_with_delay"),
        call_id: "call-nested-only-server".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    };
    assert!(router.tool_supports_parallel(&nested_only_call));

    Ok(())
}

#[tokio::test]
async fn tools_without_handlers_do_not_support_parallel() -> anyhow::Result<()> {
    let (_, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let router = test_tool_router(
        step_context.as_ref(),
        Vec::new(),
        Vec::new(),
        &turn.dynamic_tools,
    );

    assert!(!router.tool_supports_parallel(&ToolCall {
        tool_name: ToolName::plain("web_search"),
        call_id: "call-web-search".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        encrypted_function_args: None,
    }));

    Ok(())
}

#[tokio::test]
async fn specs_filter_deferred_dynamic_tools() -> anyhow::Result<()> {
    let (_, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let hidden_tool = "hidden_dynamic_tool";
    let visible_tool = "visible_dynamic_tool";
    let dynamic_tools = vec![DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
        name: "codex_app".to_string(),
        description: "Codex app tools.".to_string(),
        tools: vec![
            DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
                name: hidden_tool.to_string(),
                description: "Hidden until discovered.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
                defer_loading: true,
            }),
            DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
                name: visible_tool.to_string(),
                description: "Visible immediately.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
                defer_loading: false,
            }),
        ],
    })];

    let router = test_tool_router(
        step_context.as_ref(),
        Vec::new(),
        Vec::new(),
        &dynamic_tools,
    );
    let visible_specs = router.model_visible_specs();

    assert!(Arc::ptr_eq(&visible_specs, &router.model_visible_specs()));
    assert_eq!(
        namespace_function_names(&visible_specs, "codex_app"),
        vec![visible_tool.to_string()]
    );
    assert_eq!(
        router.deferred_tool_namespaces(),
        BTreeMap::from([("codex_app".to_string(), "Codex app tools.".to_string())])
    );

    let updated_router = test_tool_router(step_context.as_ref(), Vec::new(), Vec::new(), &[]);
    let updated_specs = updated_router.model_visible_specs();
    assert!(!Arc::ptr_eq(&visible_specs, &updated_specs));
    assert!(namespace_function_names(&updated_specs, "codex_app").is_empty());

    Ok(())
}

fn mcp_tool_info(
    server_name: &str,
    supports_parallel_tool_calls: bool,
    callable_namespace: &str,
    tool_name: &str,
) -> codex_mcp::ToolInfo {
    codex_mcp::ToolInfo {
        server_name: server_name.to_string(),
        supports_parallel_tool_calls,
        server_origin: None,
        callable_name: tool_name.to_string(),
        callable_namespace: callable_namespace.to_string(),
        namespace_description: None,
        tool: rmcp::model::Tool::new(
            tool_name.to_string(),
            "Test MCP tool",
            Arc::new(rmcp::model::object(json!({
                "type": "object",
            }))),
        ),
        openai_file_input_optional_fields: Default::default(),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn mcp_runtime(tool_info: codex_mcp::ToolInfo) -> RegisteredTool {
    let runtime = Arc::new(McpHandler::new(tool_info).expect("MCP tool spec should build"))
        as Arc<dyn CoreToolRuntime>;
    RegisteredTool {
        exposure: runtime.exposure(),
        runtime,
    }
}

#[tokio::test]
async fn extension_tool_executors_are_model_visible_and_dispatchable() -> anyhow::Result<()> {
    let (mut session, turn) = make_session_and_context().await;
    session.services.extensions = extension_tool_test_registry();
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    let history_item = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "extension history".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    session
        .record_conversation_items(&turn, std::slice::from_ref(&history_item))
        .await;
    let expected_history_item = session
        .clone_history()
        .await
        .raw_items()
        .next()
        .expect("history item")
        .clone();

    let router = test_tool_router(
        step_context.as_ref(),
        Vec::new(),
        extension_tool_executors(
            &session,
            &codex_extension_api::ExtensionData::new(turn.sub_id.clone()),
        ),
        &turn.dynamic_tools,
    );

    assert!(
        router.model_visible_specs().iter().any(
            |spec| matches!(spec, ToolSpec::Namespace(namespace)
            if namespace.name == "extension/"
                && namespace.tools.iter().any(|tool| matches!(
                    tool,
                    ResponsesApiNamespaceTool::Function(tool) if tool.name == "echo"
                )))
        ),
        "expected extension-provided tool to be visible to the model"
    );

    let call = ToolRouter::build_tool_call(ResponseItem::FunctionCall {
        id: None,
        name: "echo".to_string(),
        namespace: Some("extension/".to_string()),
        arguments: json!({ "message": "hello" }).to_string(),
        call_id: "call-extension".to_string(),
        encrypted_function_args: None,
        internal_chat_message_metadata_passthrough: None,
    })?
    .expect("function_call should produce a tool call");
    let result = router
        .dispatch_tool_call_with_code_mode_result(
            Arc::new(session),
            step_context,
            CancellationToken::new(),
            Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new())),
            call,
            ToolCallSource::Direct,
        )
        .await?;

    let response = result.into_response();
    match response {
        ResponseInputItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "call-extension");
            let FunctionCallOutputBody::Text(text) = output.body else {
                panic!("expected text function call output")
            };
            let value: serde_json::Value =
                serde_json::from_str(&text).expect("extension tool output should be json");
            assert_eq!(
                strip_response_item_ids_from_json(value),
                strip_response_item_ids_from_json(json!({
                    "arguments": { "message": "hello" },
                    "callId": "call-extension",
                    "conversationHistory": [expected_history_item],
                    "ok": true,
                }))
            );
        }
        other => panic!("expected function call output, got {other:?}"),
    }

    Ok(())
}

fn namespace_function_names(specs: &[ToolSpec], namespace_name: &str) -> Vec<String> {
    specs
        .iter()
        .find_map(|spec| match spec {
            ToolSpec::Namespace(namespace) if namespace.name == namespace_name => Some(
                namespace
                    .tools
                    .iter()
                    .map(|tool| match tool {
                        ResponsesApiNamespaceTool::Function(tool) => tool.name.clone(),
                        ResponsesApiNamespaceTool::Custom(tool) => tool.name.clone(),
                    })
                    .collect(),
            ),
            ToolSpec::Function(_)
            | ToolSpec::Freeform(_)
            | ToolSpec::ToolSearch { .. }
            | ToolSpec::WebSearch { .. }
            | ToolSpec::XSearch
            | ToolSpec::Namespace(_) => None,
        })
        .unwrap_or_default()
}
