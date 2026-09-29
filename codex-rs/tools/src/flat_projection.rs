//! Flat function projection of a finalized model-visible tool plan.
//!
//! A Provider whose Responses implementation accepts only plain `function`
//! tools cannot receive stock namespaced or custom (freeform) declarations.
//! This module rewrites such a plan into flat functions with stable,
//! collision-checked wire names and keeps the reverse routes so a call the
//! model makes on a wire name is restored to its canonical tool identity
//! before dispatch. It is a Provider tool projection in the North Star sense:
//! canonical tools stay canonical inside the harness, only the wire changes.
//!
//! Which Provider needs this is decided by the resolved model provider
//! (`ModelProvider::projects_tools_as_flat_functions`); this module has no
//! Provider-specific knowledge.

use crate::FreeformTool;
use crate::FunctionCallError;
use crate::JsonSchema;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use crate::ToolName;
use crate::ToolSpec;
use sha1::Digest;
use sha1::Sha1;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireToolRoute {
    Function(ToolName),
    Custom {
        tool_name: ToolName,
        input_key: String,
    },
}

impl WireToolRoute {
    fn tool_name(&self) -> &ToolName {
        match self {
            Self::Function(tool_name) | Self::Custom { tool_name, .. } => tool_name,
        }
    }
}

/// Compiled symbols for one finalized model-visible tool plan.
#[derive(Default)]
pub struct FlatToolRoutes {
    by_wire_name: BTreeMap<String, WireToolRoute>,
}

impl FlatToolRoutes {
    fn insert(&mut self, kind: &str, route: WireToolRoute) -> Result<String, String> {
        let wire_name = flat_wire_name(kind, route.tool_name());
        if let Some(existing) = self.by_wire_name.get(&wire_name) {
            return if existing == &route {
                Ok(wire_name)
            } else {
                Err(wire_name)
            };
        }
        self.by_wire_name.insert(wire_name.clone(), route);
        Ok(wire_name)
    }

    pub fn resolve(&self, name: &str) -> Option<&WireToolRoute> {
        self.by_wire_name.get(name)
    }

    pub fn contains_canonical(&self, tool_name: &ToolName) -> bool {
        let tool_name = tool_name.clone().with_default_namespace();
        self.by_wire_name
            .values()
            .any(|route| route.tool_name().clone().with_default_namespace() == tool_name)
    }
}

pub fn project_flat_function_tools(
    specs: Vec<ToolSpec>,
) -> Result<(Vec<ToolSpec>, FlatToolRoutes), String> {
    let mut declarations = Vec::new();
    let mut routes = FlatToolRoutes::default();
    for spec in specs {
        match spec {
            ToolSpec::Function(tool) => {
                let tool_name = ToolName::plain(tool.name.clone());
                let wire_name =
                    routes.insert("function", WireToolRoute::Function(tool_name.clone()))?;
                declarations.push(ToolSpec::Function(function_declaration(
                    wire_name, &tool_name, tool,
                )));
            }
            ToolSpec::Freeform(tool) => {
                let tool_name = ToolName::plain(tool.name.clone());
                let input_key = custom_input_key(&tool_name.name).to_string();
                let wire_name = routes.insert(
                    "custom",
                    WireToolRoute::Custom {
                        tool_name: tool_name.clone(),
                        input_key: input_key.clone(),
                    },
                )?;
                declarations.push(ToolSpec::Function(custom_function_declaration(
                    wire_name, &tool_name, tool, &input_key,
                )));
            }
            ToolSpec::Namespace(namespace) => {
                for tool in namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(tool) => {
                            let tool_name =
                                ToolName::namespaced(namespace.name.clone(), tool.name.clone());
                            let wire_name = routes
                                .insert("function", WireToolRoute::Function(tool_name.clone()))?;
                            declarations.push(ToolSpec::Function(function_declaration(
                                wire_name, &tool_name, tool,
                            )));
                        }
                        ResponsesApiNamespaceTool::Custom(tool) => {
                            let tool_name =
                                ToolName::namespaced(namespace.name.clone(), tool.name.clone());
                            let input_key = custom_input_key(&tool_name.name).to_string();
                            let wire_name = routes.insert(
                                "custom",
                                WireToolRoute::Custom {
                                    tool_name: tool_name.clone(),
                                    input_key: input_key.clone(),
                                },
                            )?;
                            declarations.push(ToolSpec::Function(custom_function_declaration(
                                wire_name, &tool_name, tool, &input_key,
                            )));
                        }
                    }
                }
            }
            hosted => declarations.push(hosted),
        }
    }
    Ok((declarations, routes))
}

fn function_declaration(
    wire_name: String,
    tool_name: &ToolName,
    mut tool: ResponsesApiTool,
) -> ResponsesApiTool {
    tool.name = wire_name;
    tool.description = flat_route_description(tool_name, &tool.description);
    tool.defer_loading = None;
    tool
}

fn custom_function_declaration(
    wire_name: String,
    tool_name: &ToolName,
    tool: FreeformTool,
    input_key: &str,
) -> ResponsesApiTool {
    let parameters = JsonSchema::object(
        BTreeMap::from([(
            input_key.to_string(),
            JsonSchema::string(Some(
                "Freeform input passed unchanged to Codex.".to_string(),
            )),
        )]),
        Some(vec![input_key.to_string()]),
        Some(false.into()),
    );
    let description = flat_route_description(
        tool_name,
        &format!("{}\n\n{}", tool.description, tool.format.definition),
    );
    ResponsesApiTool {
        name: wire_name,
        description,
        strict: true,
        defer_loading: None,
        parameters,
        output_schema: None,
    }
}

fn flat_route_description(tool_name: &ToolName, description: &str) -> String {
    let canonical_name = if tool_name.is_default_namespace() {
        tool_name.name.clone()
    } else {
        format!(
            "{}.{}",
            tool_name.namespace.as_deref().unwrap_or_default(),
            tool_name.name
        )
    };
    format!(
        "This flat Provider function directly invokes the canonical `{canonical_name}` tool. Call this function itself. Do not invoke the canonical tool through a shell, code-mode wrapper, or another tool; any such invocation guidance in the retained description does not apply to this flat interface.\n\n{description}"
    )
}

pub fn flat_wire_name(kind: &str, tool_name: &ToolName) -> String {
    // Match the stock Codex model-visible MCP tool-name budget and digest width. These are not
    // Provider limits. SHA-1 is a stock core dependency; the digest only disambiguates names.
    const MAX_FLAT_WIRE_NAME_BYTES: usize = 128;
    const WIRE_NAME_PREFIX: &str = "local__";
    const WIRE_NAME_SEPARATOR: &str = "__";
    const WIRE_ROUTE_DIGEST_HEX_CHARS: usize = 12;

    let namespace = if tool_name.is_default_namespace() {
        ""
    } else {
        tool_name.namespace.as_deref().unwrap_or_default()
    };
    let digest = format!(
        "{:x}",
        Sha1::digest(format!("{kind}\0{namespace}\0{}", tool_name.name).as_bytes())
    );
    let semantic_name = crate::code_mode_name_for_tool_name(tool_name);
    let mut semantic_name = semantic_name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    let digest = &digest[..WIRE_ROUTE_DIGEST_HEX_CHARS];
    if !semantic_name.is_empty() {
        let semantic_budget = MAX_FLAT_WIRE_NAME_BYTES
            .saturating_sub(WIRE_NAME_PREFIX.len())
            .saturating_sub(WIRE_NAME_SEPARATOR.len())
            .saturating_sub(WIRE_ROUTE_DIGEST_HEX_CHARS);
        semantic_name.truncate(semantic_budget);
        return format!("{WIRE_NAME_PREFIX}{semantic_name}{WIRE_NAME_SEPARATOR}{digest}");
    }
    format!("{WIRE_NAME_PREFIX}{digest}")
}

pub fn custom_input_key(tool_name: &str) -> &'static str {
    match tool_name {
        "apply_patch" => "patch",
        "exec" => "source",
        _ => "input",
    }
}

pub fn decode_custom_input(
    wire_name: &str,
    arguments: &str,
    input_key: &str,
) -> Result<String, FunctionCallError> {
    let value: serde_json::Value = serde_json::from_str(arguments).map_err(|error| {
        FunctionCallError::RespondToModel(format!("invalid arguments for `{wire_name}`: {error}"))
    })?;
    let input = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get(input_key))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(format!(
                "invalid arguments for `{wire_name}`: expected one string field `{input_key}`"
            ))
        })?;
    Ok(input.to_string())
}
