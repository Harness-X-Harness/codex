//! Executable pins for the stock seams the Grok flat projection depends on.
//!
//! Each pin fails loudly, at compile time where the shape is an enum, when
//! upstream changes something the Grok graft assumes silently. That turns an
//! upstream bump into an explicit review instead of a Live-only surprise.

use crate::tools::context::ToolCallSource;
use crate::tools::handlers::multi_agents_spec::SpawnAgentToolOptions;
use crate::tools::handlers::multi_agents_spec::create_spawn_agent_tool_v2;
use crate::tools::registry::ToolRegistry;
use crate::tools::router::ToolRouter;
use crate::tools::router::is_plaintext_collaboration_tool;
use codex_protocol::items::CollabAgentTool;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ToolMode;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

const COLLABORATION_NAMESPACE: &str = "collaboration";
const FLAT_WIRE_NAME_PREFIX: &str = "local__";

fn function_tool(name: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: format!("Call {name}."),
        strict: true,
        parameters: JsonSchema::object(BTreeMap::new(), None, Some(false.into())),
        output_schema: None,
        defer_loading: None,
    }
}

fn flat_router(specs: Vec<ToolSpec>) -> anyhow::Result<ToolRouter> {
    ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        specs,
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .map_err(anyhow::Error::msg)
}

/// Exhaustive on purpose: a new upstream `ToolSpec` variant must be classified
/// here before the flat projection is allowed to pass it through as hosted.
fn is_provider_hosted(spec: &ToolSpec) -> bool {
    match spec {
        ToolSpec::Function(_) | ToolSpec::Namespace(_) | ToolSpec::Freeform(_) => false,
        ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } | ToolSpec::XSearch => true,
    }
}

#[test]
fn every_tool_spec_variant_is_classified_for_flat_projection() -> anyhow::Result<()> {
    let specs = vec![
        ToolSpec::Function(function_tool("plain")),
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: "nested".to_string(),
            description: "Nested tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(function_tool("inner"))],
        }),
        ToolSpec::Freeform(FreeformTool {
            name: "exec".to_string(),
            description: "Run code.".to_string(),
            defer_loading: None,
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: /.+/".to_string(),
            },
        }),
        ToolSpec::ToolSearch {
            execution: "server".to_string(),
            description: "Search deferred tools.".to_string(),
            parameters: JsonSchema::object(BTreeMap::new(), None, Some(false.into())),
        },
        ToolSpec::WebSearch {
            external_web_access: Some(true),
            indexed_web_access: None,
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        },
        ToolSpec::XSearch,
    ];
    let hosted = specs
        .iter()
        .filter(|spec| is_provider_hosted(spec))
        .cloned()
        .collect::<Vec<_>>();
    let local_count = specs.len() - hosted.len();

    let router = flat_router(specs)?;
    let (flat, passthrough): (Vec<ToolSpec>, Vec<ToolSpec>) = router
        .model_visible_specs()
        .iter()
        .cloned()
        .partition(|spec| {
            matches!(spec, ToolSpec::Function(tool) if tool.name.starts_with(FLAT_WIRE_NAME_PREFIX))
        });

    assert_eq!(passthrough, hosted);
    assert_eq!(flat.len(), local_count);
    Ok(())
}

/// Exhaustive on purpose: every stock collaboration tool must state whether
/// Grok restores it as a plaintext direct call. Only the tools whose arguments
/// carry inter-agent message text are plaintext; the rest stay ordinary calls.
fn plaintext_expectation(tool: CollabAgentTool) -> (&'static str, bool) {
    match tool {
        CollabAgentTool::SpawnAgent => ("spawn_agent", true),
        CollabAgentTool::SendMessage => ("send_message", true),
        CollabAgentTool::FollowupTask => ("followup_task", true),
        CollabAgentTool::SendInput => ("send_input", false),
        CollabAgentTool::ResumeAgent => ("resume_agent", false),
        CollabAgentTool::Wait => ("wait_agent", false),
        CollabAgentTool::CloseAgent => ("close_agent", false),
        CollabAgentTool::InterruptAgent => ("interrupt_agent", false),
        CollabAgentTool::ListAgents => ("list_agents", false),
    }
}

#[test]
fn plaintext_collaboration_routing_covers_every_stock_collab_tool() {
    let tools = [
        CollabAgentTool::SpawnAgent,
        CollabAgentTool::SendInput,
        CollabAgentTool::ResumeAgent,
        CollabAgentTool::Wait,
        CollabAgentTool::CloseAgent,
        CollabAgentTool::SendMessage,
        CollabAgentTool::FollowupTask,
        CollabAgentTool::InterruptAgent,
        CollabAgentTool::ListAgents,
    ];
    let expected = tools.map(plaintext_expectation);
    let observed = tools.map(|tool| {
        let (name, _) = plaintext_expectation(tool);
        (
            name,
            is_plaintext_collaboration_tool(&ToolName::namespaced(COLLABORATION_NAMESPACE, name)),
        )
    });

    assert_eq!(observed, expected);
    assert!(
        !is_plaintext_collaboration_tool(&ToolName::plain("spawn_agent")),
        "plaintext marking is scoped to the collaboration namespace"
    );
}

#[test]
fn spawn_agent_flat_projection_keeps_the_stock_argument_contract() -> anyhow::Result<()> {
    let ToolSpec::Function(stock_spawn) =
        create_spawn_agent_tool_v2(SpawnAgentToolOptions::default())
    else {
        panic!("stock spawn_agent V2 should be a function tool");
    };
    let router = flat_router(vec![ToolSpec::Namespace(ResponsesApiNamespace {
        name: COLLABORATION_NAMESPACE.to_string(),
        description: "Agent tools.".to_string(),
        tools: vec![ResponsesApiNamespaceTool::Function(stock_spawn)],
    })])?;
    let ToolSpec::Function(projected) = &router.model_visible_specs()[0] else {
        panic!("spawn_agent should project to a flat function");
    };

    // The Live collaboration scenario and the gateway-compatibility suite send
    // exactly `{message, task_name}`; the projected schema must still require it.
    let parameters = serde_json::to_value(&projected.parameters)?;
    assert_eq!(parameters["required"], json!(["task_name", "message"]));
    assert_eq!(parameters["properties"]["message"]["type"], json!("string"));
    assert_eq!(
        parameters["properties"]["task_name"]["type"],
        json!("string")
    );

    let mut item = ResponseItem::FunctionCall {
        id: None,
        name: projected.name.clone(),
        namespace: None,
        arguments: json!({"message": "first worker task", "task_name": "first"}).to_string(),
        encrypted_function_args: None,
        call_id: "spawn-call".to_string(),
        internal_chat_message_metadata_passthrough: None,
    };
    router.restore_tool_call(&mut item)?;
    let call = ToolRouter::build_tool_call(item)?.expect("restored spawn should be a tool call");

    assert_eq!(
        call.tool_name,
        ToolName::namespaced(COLLABORATION_NAMESPACE, "spawn_agent")
    );
    assert_eq!(call.direct_source(), ToolCallSource::DirectPlaintextMessage);
    Ok(())
}
