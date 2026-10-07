//! Flat wire declarations and plan-bound reverse routes for local tools.
//! Canonical tool definitions and history are never mutated.

use crate::FreeformTool;
use crate::FunctionCallError;
use crate::JsonSchema;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use crate::ToolName;
use crate::ToolSpec;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
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
            Self::Function(name)
            | Self::Custom {
                tool_name: name, ..
            } => name,
        }
    }

    fn wire_name(&self) -> String {
        let kind = match self {
            Self::Function(_) => "function",
            Self::Custom { .. } => "custom",
        };
        flat_wire_name(kind, self.tool_name())
    }
}

#[derive(Clone, Debug, Default)]
pub struct FlatToolRoutes {
    by_wire_name: BTreeMap<String, WireToolRoute>,
}

impl FlatToolRoutes {
    pub fn resolve(&self, name: &str) -> Option<&WireToolRoute> {
        self.by_wire_name.get(name)
    }

    pub(super) fn remember(&mut self, route: WireToolRoute) -> Result<String, String> {
        let wire_name = route.wire_name();
        if let Some(existing) = self.by_wire_name.get(&wire_name)
            && existing != &route
        {
            return Err(format!("flat tool name collision: {wire_name}"));
        }
        self.by_wire_name.insert(wire_name.clone(), route);
        Ok(wire_name)
    }

    /// Restore a completed wire call using only this finalized plan's symbols.
    /// Partial streamed calls must not be passed here: custom arguments must be complete.
    pub fn restore_response_item(&self, item: ResponseItem) -> Result<ResponseItem, String> {
        match item {
            ResponseItem::FunctionCall {
                id,
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                internal_chat_message_metadata_passthrough,
            } => {
                if encrypted_function_args.is_some() {
                    return Err("flat function call cannot contain encrypted arguments".to_string());
                }
                if !ToolName::new(namespace, &name).is_default_namespace() {
                    return Err("flat function call has a non-default namespace".to_string());
                }
                match self
                    .resolve(&name)
                    .ok_or_else(|| format!("unknown flat tool: {name}"))?
                {
                    WireToolRoute::Function(tool_name) => Ok(ResponseItem::FunctionCall {
                        id,
                        name: tool_name.name.clone(),
                        namespace: tool_name.namespace.clone(),
                        arguments,
                        encrypted_function_args,
                        call_id,
                        internal_chat_message_metadata_passthrough,
                    }),
                    WireToolRoute::Custom {
                        tool_name,
                        input_key,
                    } => {
                        let input = decode_custom_input(&name, &arguments, input_key)
                            .map_err(|error| error.to_string())?;
                        Ok(ResponseItem::CustomToolCall {
                            id,
                            status: None,
                            call_id,
                            name: tool_name.name.clone(),
                            namespace: tool_name.namespace.clone(),
                            input,
                            internal_chat_message_metadata_passthrough,
                        })
                    }
                }
            }
            ResponseItem::CustomToolCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::ImageGenerationCall { .. } => {
                Err("expected a flat function call, received an unsupported tool call".to_string())
            }
            item => Ok(item),
        }
    }
}

pub fn project_flat_function_tools(
    specs: &[ToolSpec],
) -> Result<(Vec<ToolSpec>, FlatToolRoutes), String> {
    let mut declarations = Vec::new();
    let mut routes = FlatToolRoutes::default();
    for spec in specs {
        let tools = match spec {
            ToolSpec::Function(tool) => {
                vec![(None, ResponsesApiNamespaceTool::Function(tool.clone()))]
            }
            ToolSpec::Freeform(tool) => {
                vec![(None, ResponsesApiNamespaceTool::Custom(tool.clone()))]
            }
            ToolSpec::Namespace(namespace) => namespace
                .tools
                .iter()
                .cloned()
                .map(|tool| (Some(namespace.name.clone()), tool))
                .collect(),
            ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } => {
                return Err(format!("flat local tools do not support {}", spec.name()));
            }
        };
        for (namespace, tool) in tools {
            let (route, mut declaration) = match tool {
                ResponsesApiNamespaceTool::Function(tool) => (
                    WireToolRoute::Function(normalize_name(ToolName::new(namespace, &tool.name))),
                    tool,
                ),
                ResponsesApiNamespaceTool::Custom(tool) => {
                    let name = normalize_name(ToolName::new(namespace, &tool.name));
                    let input_key = custom_input_key(&name).to_string();
                    let declaration = custom_declaration(tool, &input_key);
                    (
                        WireToolRoute::Custom {
                            tool_name: name,
                            input_key,
                        },
                        declaration,
                    )
                }
            };
            let wire_name = route.wire_name();
            if routes.resolve(&wire_name) == Some(&route) {
                return Err(format!("duplicate flat tool declaration: {wire_name}"));
            }
            declaration.name = routes.remember(route.clone())?;
            declaration.defer_loading = None;
            let name = route.tool_name();
            let canonical = match &name.namespace {
                Some(namespace) => format!("{namespace}.{}", name.name),
                None => name.name.clone(),
            };
            declaration.description = format!(
                "Call this function directly to invoke `{canonical}`. Do not invoke it through another tool.\n\n{}",
                declaration.description,
            );
            declarations.push(ToolSpec::Function(declaration));
        }
    }
    Ok((declarations, routes))
}

fn custom_declaration(tool: FreeformTool, input_key: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: tool.name,
        description: format!("{}\n\n{}", tool.description, tool.format.definition),
        strict: true,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::from([(
                input_key.to_string(),
                JsonSchema::string(Some(
                    "Freeform input passed unchanged to the tool.".to_string(),
                )),
            )]),
            Some(vec![input_key.to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    }
}

pub(super) fn normalize_name(mut name: ToolName) -> ToolName {
    if name.is_default_namespace() {
        name.namespace = None;
    }
    name
}

pub(super) fn custom_route(name: ToolName) -> WireToolRoute {
    let name = normalize_name(name);
    let input_key = custom_input_key(&name).to_string();
    WireToolRoute::Custom {
        tool_name: name,
        input_key,
    }
}

fn custom_input_key(name: &ToolName) -> &'static str {
    match (name.is_default_namespace(), name.name.as_str()) {
        (true, "apply_patch") => "patch",
        (true, "exec") => "source",
        _ => "input",
    }
}

/// Stable, readable 128-byte symbols. The digest is only a disambiguator;
/// every use checks collisions against the request's complete route map.
pub fn flat_wire_name(kind: &str, name: &ToolName) -> String {
    let name = normalize_name(name.clone());
    let namespace = name.namespace.as_deref().unwrap_or_default();
    // Length-prefixed UTF-8 fields avoid separator and namespace ambiguities.
    let mut hash = 0xcbf29ce484222325_u64;
    for field in [kind, namespace, name.name.as_str()] {
        for byte in (field.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(field.bytes())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let semantic = format!("{namespace}_{}", name.name);
    let semantic: String = semantic
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                char::from(byte)
            } else {
                '_'
            }
        })
        .take(104)
        .collect();
    format!("local__{semantic}_{hash:016x}")
}

pub fn decode_custom_input(
    wire_name: &str,
    arguments: &str,
    input_key: &str,
) -> Result<String, FunctionCallError> {
    // A map visitor rejects duplicate keys too, unlike serde_json::Value.
    struct SingleStringField(String, String);
    impl<'de> Deserialize<'de> for SingleStringField {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = SingleStringField;
                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("exactly one string field")
                }
                fn visit_map<M: serde::de::MapAccess<'de>>(
                    self,
                    mut map: M,
                ) -> Result<Self::Value, M::Error> {
                    let (key, value) = map
                        .next_entry::<String, String>()?
                        .ok_or_else(|| serde::de::Error::custom("missing input field"))?;
                    if map.next_entry::<String, serde::de::IgnoredAny>()?.is_some() {
                        return Err(serde::de::Error::custom("extra or duplicate input field"));
                    }
                    Ok(SingleStringField(key, value))
                }
            }
            deserializer.deserialize_map(Visitor)
        }
    }
    let field: SingleStringField = serde_json::from_str(arguments).map_err(|_| {
        FunctionCallError::RespondToModel(format!(
            "invalid arguments for `{wire_name}`: expected one string field `{input_key}`"
        ))
    })?;
    if field.0 != input_key {
        return Err(FunctionCallError::RespondToModel(format!(
            "invalid arguments for `{wire_name}`: expected field `{input_key}`"
        )));
    }
    Ok(field.1)
}

#[cfg(test)]
#[path = "flat_projection_tests.rs"]
mod tests;
