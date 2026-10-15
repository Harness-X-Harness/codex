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
use codex_protocol::models::ResponseItem;
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
            .tool_runtime(&ToolName::plain(parallel_tool_name))
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
            .tool_runtime(&call.tool_name)
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
            .tool_runtime(&different_server_call.tool_name)
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
    assert!(router.tool_runtime(&hidden_call.tool_name).is_some());

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
        .record_conversation_items(
            &turn,
            turn.model_info(),
            std::slice::from_ref(&history_item),
        )
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

    let response = result.into_response().item;
    match response {
        ResponseItem::FunctionCallOutput {
            call_id, output, ..
        } => {
            assert_eq!(call_id.as_deref(), Some("call-extension"));
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
            | ToolSpec::Namespace(_) => None,
        })
        .unwrap_or_default()
}

fn flat_router(specs: Vec<ToolSpec>) -> ToolRouter {
    let (_, routes) = codex_tools::project_flat_function_tools(&specs).unwrap();
    let mut router = ToolRouter::from_parts(
        crate::tools::registry::ToolRegistry::default(),
        specs,
        codex_protocol::openai_models::ToolMode::Direct,
        BTreeMap::new(),
        /*tool_namespaces_info*/ None,
        &[],
    );
    router.flat_tool_routes = Some(routes);
    router
}

fn flat_call(name: &ToolName, kind: &str, arguments: &str) -> ResponseItem {
    serde_json::from_value(json!({
        "type": "function_call",
        "id": "fc_flat",
        "call_id": "call_flat",
        "name": codex_tools::flat_wire_name(kind, name),
        "arguments": arguments,
    }))
    .unwrap()
}

fn custom_patch_spec(namespace: Option<&str>) -> ToolSpec {
    let tool = codex_tools::FreeformTool {
        name: "apply_patch".into(),
        description: "Apply a patch.".into(),
        defer_loading: None,
        format: codex_tools::FreeformToolFormat {
            r#type: "grammar".into(),
            syntax: "lark".into(),
            definition: "start: /[\\s\\S]+/".into(),
        },
    };
    match namespace {
        None => ToolSpec::Freeform(tool),
        Some(namespace) => ToolSpec::Namespace(ResponsesApiNamespace {
            name: namespace.into(),
            description: "External tools.".into(),
            tools: vec![ResponsesApiNamespaceTool::Custom(tool)],
        }),
    }
}

#[test]
fn flat_router_restores_exact_function_arguments_and_keeps_canonical_exposure() {
    use crate::client_common::ResponseEvent;
    let spec = ExtensionEchoExecutor.spec();
    let router = flat_router(vec![spec.clone()]);
    let name = ToolName::namespaced("extension/", "echo");
    let arguments = "{\"n\": 9007199254740993, \"message\":\"exact\"}";
    let call = flat_call(&name, "function", arguments);
    let Some(ResponseEvent::OutputItemDone(restored)) = router
        .normalize_response_event(ResponseEvent::OutputItemDone(call.clone()))
        .unwrap()
    else {
        panic!("expected restored completed call");
    };
    let expected: ResponseItem = serde_json::from_value(json!({
        "type": "function_call", "id": "fc_flat", "call_id": "call_flat",
        "namespace": "extension/", "name": "echo", "arguments": arguments,
    }))
    .unwrap();
    assert_eq!(restored, expected);
    assert_eq!(router.model_visible_specs().as_ref(), &[spec]);
    assert!(router.exposes_tool(&name));
    assert!(
        router
            .normalize_response_event(ResponseEvent::OutputItemAdded(call))
            .unwrap()
            .is_none()
    );
    // A newer plan cannot authorize a symbol absent from this captured router.
    let newer = flat_router(vec![]);
    assert!(
        newer
            .normalize_response_event(ResponseEvent::OutputItemDone(flat_call(
                &name, "function", arguments
            ),))
            .is_err()
    );
}

#[test]
fn flat_router_checks_patch_grammar_only_for_the_canonical_builtin() {
    use crate::client_common::ResponseEvent;
    let name = ToolName::plain("apply_patch");
    let router = flat_router(vec![custom_patch_spec(/*namespace*/ None)]);
    for patch in [
        "echo unsafe",
        "*** Begin Patch\ninvalid hunk\n*** End Patch",
        "<<'EOF'\n*** Begin Patch\n*** End Patch\nEOF",
    ] {
        let call = flat_call(&name, "custom", &json!({"patch": patch}).to_string());
        assert!(
            router
                .normalize_response_event(ResponseEvent::OutputItemDone(call))
                .is_err()
        );
    }
    let patch = "*** Begin Patch\n*** Add File: example.txt\n+hello\n*** End Patch\n";
    let call = flat_call(&name, "custom", &json!({"patch": patch}).to_string());
    let Some(ResponseEvent::OutputItemDone(restored)) = router
        .normalize_response_event(ResponseEvent::OutputItemDone(call))
        .unwrap()
    else {
        panic!("expected custom call");
    };
    let expected: ResponseItem = serde_json::from_value(json!({
        "type": "custom_tool_call", "id": "fc_flat", "call_id": "call_flat",
        "name": "apply_patch", "input": patch,
    }))
    .unwrap();
    assert_eq!(restored, expected);
    let external = flat_router(vec![custom_patch_spec(Some("external"))]);
    let call = flat_call(
        &ToolName::namespaced("external", "apply_patch"),
        "custom",
        r#"{"input":"external grammar"}"#,
    );
    assert!(
        external
            .normalize_response_event(ResponseEvent::OutputItemDone(call))
            .is_ok()
    );
}

#[test]
fn flat_collaboration_calls_keep_configured_plaintext_logging_redaction() {
    use crate::client_common::ResponseEvent;
    for namespace in [Some("collaboration"), Some("agents"), None] {
        for name in ["spawn_agent", "send_message", "followup_task"] {
            let ToolSpec::Namespace(mut spec) = ExtensionEchoExecutor.spec() else {
                unreachable!()
            };
            spec.name = namespace.unwrap_or("functions").into();
            let ResponsesApiNamespaceTool::Function(function) = &mut spec.tools[0] else {
                unreachable!()
            };
            function.name = name.into();
            let mut router = flat_router(vec![ToolSpec::Namespace(spec)]);
            let name = ToolName::new(namespace.map(str::to_string), name);
            router
                .plaintext_collaboration_tools
                .insert(name.clone().with_default_namespace());
            let item = flat_call(&name, "function", r#"{"message":"secret"}"#);
            let Some(ResponseEvent::OutputItemDone(restored)) = router
                .normalize_response_event(ResponseEvent::OutputItemDone(item))
                .unwrap()
            else {
                panic!("expected restored collaboration call");
            };
            let mut call = ToolRouter::build_tool_call(restored).unwrap().unwrap();
            assert_eq!(call.encrypted_function_args, None);
            assert_eq!(
                router.direct_source(&call),
                ToolCallSource::DirectPlaintextMessage
            );
            assert_eq!(
                tool_log_payload(&call.payload, &router.direct_source(&call)),
                "[plaintext arguments]"
            );

            // Identity alone cannot turn an unrelated custom payload into a message.
            call.payload = ToolPayload::Custom {
                input: "custom input".into(),
            };
            assert_eq!(router.direct_source(&call), ToolCallSource::Direct);
            call.payload = ToolPayload::Function {
                arguments: "{}".into(),
            };
            call.tool_name = ToolName::namespaced("unrelated", name.name);
            assert_eq!(router.direct_source(&call), ToolCallSource::Direct);
        }
    }
}

#[test]
fn stock_direct_source_keeps_existing_encrypted_marker_behavior() {
    let router = ToolRouter::from_parts(
        crate::tools::registry::ToolRegistry::default(),
        Vec::new(),
        codex_protocol::openai_models::ToolMode::Direct,
        BTreeMap::new(),
        /*tool_namespaces_info*/ None,
        &[],
    );
    for namespace in ["collaboration", "agents", "functions"] {
        for encrypted_function_args in [None, Some(Vec::new()), Some(vec!["cipher".into()])] {
            let call = ToolCall {
                tool_name: ToolName::namespaced(namespace, "send_message"),
                call_id: "stock".into(),
                payload: ToolPayload::Function {
                    arguments: "{}".into(),
                },
                encrypted_function_args,
            };
            assert_eq!(router.direct_source(&call), call.direct_source());
        }
    }
}

#[test]
fn stock_router_does_not_decode_flat_looking_calls() {
    use crate::client_common::ResponseEvent;
    let mut router = flat_router(vec![]);
    router.flat_tool_routes = None;
    let original = flat_call(&ToolName::plain("apply_patch"), "custom", "not JSON");
    let Some(ResponseEvent::OutputItemDone(actual)) = router
        .normalize_response_event(ResponseEvent::OutputItemDone(original.clone()))
        .unwrap()
    else {
        panic!("expected stock pass-through");
    };
    assert_eq!(actual, original);
}

#[test]
fn grok_x_activity_never_activates_a_same_named_local_custom_consumer() {
    use crate::client_common::ResponseEvent;
    let ToolSpec::Freeform(mut local_spec) = custom_patch_spec(/*namespace*/ None) else {
        unreachable!()
    };
    local_spec.name = "x_keyword_search".into();
    let router = flat_router(vec![ToolSpec::Freeform(local_spec)]);
    let hosted:ResponseItem=serde_json::from_value(json!({"type":"custom_tool_call","id":"x","call_id":"shared-call","name":"x_keyword_search","status":"in_progress","input":"hosted input"})).unwrap();
    assert!(
        router
            .normalize_response_event(ResponseEvent::OutputItemAdded(hosted.clone()))
            .unwrap()
            .is_none()
    );
    // A local call with exactly the same name still restores and builds its real
    // custom invocation from the advertised function wrapper, without hosted status.
    let local = flat_call(
        &ToolName::plain("x_keyword_search"),
        "custom",
        r#"{"input":"local exact input"}"#,
    );
    let Some(ResponseEvent::OutputItemDone(local)) = router
        .normalize_response_event(ResponseEvent::OutputItemDone(local))
        .unwrap()
    else {
        panic!("local completion")
    };
    assert!(!codex_protocol::grok_hosted::is_completed_x_search(&local));
    let call = ToolRouter::build_tool_call(local).unwrap().unwrap();
    assert_eq!(
        call.tool_name,
        ToolName::plain("x_keyword_search").with_default_namespace()
    );
    assert!(matches!(call.payload,ToolPayload::Custom {input} if input=="local exact input"));
    let empty = flat_router(vec![]);
    assert!(matches!(
        empty
            .normalize_response_event(ResponseEvent::OutputItemAdded(hosted.clone()))
            .unwrap(),
        Some(ResponseEvent::OutputItemAdded(_))
    ));
    let mut stock = router;
    stock.flat_tool_routes = None;
    let Some(ResponseEvent::OutputItemAdded(actual)) = stock
        .normalize_response_event(ResponseEvent::OutputItemAdded(hosted.clone()))
        .unwrap()
    else {
        panic!("stock custom start")
    };
    assert_eq!(actual, hosted);
}
