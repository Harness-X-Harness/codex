use std::collections::BTreeMap;
use std::sync::Arc;

use crate::config::Config;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::tools::context::ToolPayload;
use crate::tools::grok_hosted_output::GrokHostedOutput;
use crate::tools::grok_hosted_output::GrokHostedOutputEventPhase;
use crate::tools::grok_hosted_output::GrokHostedOutputOwner;
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
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::GrokLocalInputProjection;
use codex_tools::GrokLocalTool;
use codex_tools::GrokLocalToolRoute;
use codex_tools::GrokToolPlan;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::default_namespace_description;
use codex_tools::plan_grok_tools;
use core_test_support::responses::strip_response_item_ids_from_json;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::ToolCall;
use super::ToolCallSource;
use super::ToolRouter;
use super::tool_log_payload;

fn grok_router_with_x_search() -> ToolRouter {
    ToolRouter::from_grok_plan(
        Default::default(),
        GrokToolPlan {
            declarations: vec![ToolSpec::XSearch],
            local_routes: Default::default(),
        },
    )
}

fn grok_router_with(declarations: Vec<ToolSpec>) -> ToolRouter {
    ToolRouter::from_grok_plan(
        Default::default(),
        GrokToolPlan {
            declarations,
            local_routes: Default::default(),
        },
    )
}

fn grok_custom_call(name: &str) -> ResponseItem {
    ResponseItem::CustomToolCall {
        id: Some(codex_protocol::ResponseItemId::with_suffix("ct", "x")),
        status: Some("completed".to_string()),
        call_id: "call_x".to_string(),
        name: name.to_string(),
        namespace: None,
        input: r#"{"query":"Codex"}"#.to_string(),
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn grok_declared_x_search_custom_call_never_routes_to_local_dispatch() {
    let router = grok_router_with_x_search();

    assert_eq!(
        router
            .route_tool_call(grok_custom_call("x_semantic_search"))
            .expect("evidence-backed X Search should be remote-owned"),
        None
    );
}

#[test]
fn grok_x_search_projection_requires_the_current_authoritative_plan() {
    let declared = grok_router_with_x_search();
    let undeclared = grok_router_with(Vec::new());
    let mut in_progress = grok_custom_call("x_keyword_search");
    let ResponseItem::CustomToolCall { status, .. } = &mut in_progress else {
        unreachable!("fixture is a custom tool call");
    };
    *status = Some("in_progress".to_string());

    assert!(matches!(
        declared.classify_grok_hosted_output(&in_progress, GrokHostedOutputEventPhase::Added,),
        Ok(GrokHostedOutput::Hosted {
            item_id: "ct_x",
            owner: GrokHostedOutputOwner::XSearch,
            projects_custom_output: true,
        })
    ));
    assert!(matches!(
        undeclared.classify_grok_hosted_output(
            &in_progress,
            GrokHostedOutputEventPhase::Added,
        ),
        Ok(GrokHostedOutput::UnknownCustom {
            item_id: Some("ct_x"),
            reason,
        }) if reason.contains("was not declared")
    ));
}

#[test]
fn grok_x_search_projection_rejects_invalid_gateway_shape() {
    let router = grok_router_with_x_search();
    let mut namespaced = grok_custom_call("x_keyword_search");
    let ResponseItem::CustomToolCall { namespace, .. } = &mut namespaced else {
        unreachable!("fixture is a custom tool call");
    };
    *namespace = Some("local".to_string());
    let mut missing_call_id = grok_custom_call("x_keyword_search");
    let ResponseItem::CustomToolCall { call_id, .. } = &mut missing_call_id else {
        unreachable!("fixture is a custom tool call");
    };
    call_id.clear();

    for item in [&namespaced, &missing_call_id] {
        assert!(matches!(
            router.classify_grok_hosted_output(item, GrokHostedOutputEventPhase::Added),
            Err(codex_tools::FunctionCallError::Fatal(_))
        ));
        assert!(matches!(
            router.classify_grok_hosted_output(item, GrokHostedOutputEventPhase::Done),
            Ok(GrokHostedOutput::UnknownCustom { .. })
        ));
    }
}

#[test]
fn grok_x_search_started_item_requires_the_complete_evidence_backed_shape() {
    let router = grok_router_with_x_search();
    let mut partial = grok_custom_call("x_keyword_search");
    let ResponseItem::CustomToolCall {
        id,
        status,
        call_id,
        ..
    } = &mut partial
    else {
        unreachable!("fixture is a custom tool call");
    };
    *id = None;
    *status = Some("in_progress".to_string());
    call_id.clear();

    assert!(matches!(
        router.classify_grok_hosted_output(&partial, GrokHostedOutputEventPhase::Added),
        Err(codex_tools::FunctionCallError::Fatal(message))
            if message.contains("missing provider item_id")
    ));
}

#[test]
fn grok_web_and_image_output_require_the_exact_active_declaration() {
    let web = ResponseItem::WebSearchCall {
        id: Some(codex_protocol::ResponseItemId::with_suffix(
            "ws", "declared",
        )),
        status: Some("failed".to_string()),
        action: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let image = ResponseItem::GrokImageGenerationCall {
        id: Some(codex_protocol::ResponseItemId::with_suffix(
            "ig", "declared",
        )),
        status: "failed".to_string(),
        prompt: Some("Draw a fox.".to_string()),
        result: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let web_router = grok_router_with(vec![ToolSpec::WebSearch {
        external_web_access: None,
        indexed_web_access: None,
        filters: None,
        user_location: None,
        search_context_size: None,
        search_content_types: None,
    }]);
    let image_router = grok_router_with(vec![ToolSpec::ImageGeneration]);
    let undeclared = grok_router_with(Vec::new());

    assert!(matches!(
        web_router.classify_grok_hosted_output(&web, GrokHostedOutputEventPhase::Done),
        Ok(GrokHostedOutput::Hosted {
            item_id: "ws_declared",
            owner: GrokHostedOutputOwner::WebSearch,
            projects_custom_output: false,
        })
    ));
    assert!(matches!(
        image_router.classify_grok_hosted_output(&image, GrokHostedOutputEventPhase::Done),
        Ok(GrokHostedOutput::Hosted {
            item_id: "ig_declared",
            owner: GrokHostedOutputOwner::ImageGeneration,
            projects_custom_output: false,
        })
    ));
    for item in [&web, &image] {
        assert!(matches!(
            undeclared.classify_grok_hosted_output(item, GrokHostedOutputEventPhase::Done),
            Err(codex_tools::FunctionCallError::Fatal(message))
                if message.contains("was not declared")
        ));
    }
}

#[test]
fn grok_unknown_custom_output_fails_closed() {
    let router = grok_router_with_x_search();
    let item = grok_custom_call("unknown_remote_tool");
    let mut added = item.clone();
    let ResponseItem::CustomToolCall { status, .. } = &mut added else {
        unreachable!("fixture is a custom tool call");
    };
    *status = Some("in_progress".to_string());

    assert!(matches!(
        router.classify_grok_hosted_output(&added, GrokHostedOutputEventPhase::Added),
        Ok(GrokHostedOutput::UnknownCustom {
            item_id: Some("ct_x"),
            reason,
        }) if reason.contains("unknown Grok hosted custom output")
    ));
    assert!(matches!(
        router.classify_grok_hosted_output(&item, GrokHostedOutputEventPhase::Done),
        Ok(GrokHostedOutput::UnknownCustom { reason, .. })
            if reason.contains("unknown Grok hosted custom output")
    ));

    let error = router
        .route_tool_call(item)
        .expect_err("unknown custom output must not reach local dispatch");
    assert!(matches!(
        error,
        codex_tools::FunctionCallError::Fatal(message)
            if message.contains("unknown Grok hosted custom output")
    ));
}

#[test]
fn grok_freeform_planned_entry_owns_description_identity_and_exact_decoder() {
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::namespaced("extension", "render"),
        spec: ToolSpec::Namespace(ResponsesApiNamespace {
            name: "extension".to_string(),
            description: "Extension tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Custom(FreeformTool {
                name: "render".to_string(),
                description: "Render exact source locally.".to_string(),
                defer_loading: None,
                format: FreeformToolFormat {
                    r#type: "grammar".to_string(),
                    syntax: "lark".to_string(),
                    definition: "start: /.+/s".to_string(),
                },
            })],
        }),
    }])
    .expect("namespaced freeform tool should produce one reversible planned entry");
    let ToolSpec::Function(declaration) = &plan.declarations[0] else {
        panic!("Grok wire declaration must be one function");
    };
    assert!(
        declaration
            .description
            .starts_with("Render exact source locally.")
    );
    assert!(
        declaration
            .description
            .contains(r#"{"type":"grammar","syntax":"lark","definition":"start: /.+/s"}"#)
    );
    let wire_name = declaration.name.clone();
    let router = ToolRouter::from_grok_plan(Default::default(), plan);
    let exact_input = "第一行\n\"quoted\"\\tail";

    let call = router
        .route_tool_call(ResponseItem::FunctionCall {
            id: None,
            name: wire_name,
            namespace: None,
            arguments: serde_json::json!({"input": exact_input}).to_string(),
            encrypted_function_args: None,
            call_id: "freeform-1".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })
        .expect("planned function must decode")
        .expect("planned freeform function is Local Codex-owned");

    assert_eq!(call.tool_name, ToolName::namespaced("extension", "render"));
    assert_eq!(
        call.payload,
        ToolPayload::Custom {
            input: exact_input.to_string(),
        }
    );
}

#[test]
fn grok_collaboration_function_arguments_are_plaintext() {
    let wire_name = "local__collaboration_spawn_agent__test";
    let router = ToolRouter::from_grok_plan(
        Default::default(),
        GrokToolPlan {
            declarations: Vec::new(),
            local_routes: BTreeMap::from([(
                wire_name.to_string(),
                GrokLocalToolRoute {
                    canonical_identity: ToolName::namespaced("collaboration", "spawn_agent"),
                    input_projection: GrokLocalInputProjection::Function,
                },
            )]),
        },
    );

    let call = router
        .route_tool_call(ResponseItem::FunctionCall {
            id: None,
            name: wire_name.to_string(),
            namespace: None,
            arguments: json!({"message": "review"}).to_string(),
            encrypted_function_args: None,
            call_id: "spawn-1".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })
        .expect("Grok collaboration call should decode")
        .expect("Grok collaboration call should be local-owned");

    assert_eq!(call.direct_source(), ToolCallSource::DirectPlaintextMessage);
}

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
    ) -> Vec<Arc<dyn ToolExecutor<ExtensionToolCall>>> {
        vec![Arc::new(ExtensionEchoExecutor)]
    }
}

struct ExtensionEchoExecutor;

impl ToolExecutor<ExtensionToolCall> for ExtensionEchoExecutor {
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

    fn handle(&self, call: ExtensionToolCall) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl ExtensionEchoExecutor {
    async fn handle_call(
        &self,
        call: ExtensionToolCall,
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
    extension_tool_executors: impl IntoIterator<Item = Arc<dyn ToolExecutor<ExtensionToolCall>>>,
    dynamic_tools: &[DynamicToolSpec],
) -> ToolRouter {
    let mut registry = build_core_tool_registry(
        step_context.turn.as_ref(),
        &step_context.environments,
        step_context.mcp.as_ref(),
        /*tool_suggest_candidates*/ None,
        /*wait_for_environment_tool_config*/ None,
    );
    let hosted_specs = append_source_tools(
        step_context.turn.as_ref(),
        &mut registry,
        mcp_tools,
        extension_tool_executors,
        dynamic_tools,
    );
    ToolRouter::from_registry(
        step_context.turn.as_ref(),
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

    let parallel_tool_name = ["exec_command", "shell_command"]
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

#[test]
fn grok_stable_function_route_preserves_canonical_parallel_capability() -> anyhow::Result<()> {
    let registered = mcp_runtime(mcp_tool_info(
        "echo",
        /*supports_parallel_tool_calls*/ true,
        "mcp__echo__",
        "query_with_delay",
    ));
    let identity = registered.runtime.tool_name();
    let local_spec = registered.runtime.spec();
    let registry = crate::tools::registry::ToolRegistry::from_tools([registered.runtime]);
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: identity.clone(),
        spec: local_spec,
    }])?;
    let wire_name = plan
        .declarations
        .first()
        .expect("planned declaration")
        .name()
        .to_string();
    let router = ToolRouter::from_grok_plan(registry, plan);

    let call = router
        .route_tool_call(ResponseItem::FunctionCall {
            id: None,
            name: wire_name,
            namespace: None,
            arguments: "{}".to_string(),
            encrypted_function_args: None,
            call_id: "call_grok_parallel".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })?
        .expect("Grok function should route to canonical local tool");

    assert_eq!(call.tool_name, identity);
    assert!(router.tool_supports_parallel(&call));
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

    assert_eq!(
        namespace_function_names(&router.model_visible_specs(), "codex_app"),
        vec![visible_tool.to_string()]
    );
    assert_eq!(
        router.deferred_tool_namespaces(),
        BTreeMap::from([("codex_app".to_string(), "Codex app tools.".to_string())])
    );

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
    let mut expected_history_item = history_item.clone();
    expected_history_item.set_turn_id_if_missing(&turn.sub_id);

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
            | ToolSpec::ImageGeneration
            | ToolSpec::Namespace(_) => None,
        })
        .unwrap_or_default()
}
