use crate::FreeformTool;
use crate::JsonSchema;
use crate::JsonSchemaPrimitiveType;
use crate::JsonSchemaType;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use crate::ToolName;
use crate::ToolSpec;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use thiserror::Error;

const DERIVED_NAME_PREFIX: &str = "local__";
const HASH_VERSION: &str = "grok-tool-name-v1";
const MAX_WIRE_NAME_LEN: usize = 64;
const HASH_SUFFIX_LEN: usize = 16;
const FREEFORM_FORMAT_DESCRIPTION_HEADER: &str = "Local Codex freeform grammar metadata (descriptive only; the Grok Gateway does not enforce this grammar):";
const RESERVED_WIRE_NAMES: &[&str] = &[
    "code_execution",
    "code_interpreter",
    "collections_search",
    "file_search",
    "image_generation",
    "mcp",
    "shell",
    "tool_search",
    "web_search",
    "x_search",
];

#[derive(Clone, Debug, PartialEq)]
pub struct GrokLocalTool {
    pub identity: ToolName,
    pub spec: ToolSpec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrokLocalInputProjection {
    Function,
    FunctionEnvelope { input_key: String },
    Freeform { input_key: String },
    ToolSearch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokLocalToolRoute {
    pub canonical_identity: ToolName,
    pub input_projection: GrokLocalInputProjection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GrokToolPlan {
    pub declarations: Vec<ToolSpec>,
    pub local_routes: BTreeMap<String, GrokLocalToolRoute>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrokLocalToolInput {
    FunctionArguments(String),
    Freeform(String),
    ToolSearchArguments(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokDecodedLocalToolCall {
    pub canonical_identity: ToolName,
    pub input: GrokLocalToolInput,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GrokToolPlanError {
    #[error("Grok cannot project local tool `{identity}`: {reason}")]
    UnsupportedLocalTool { identity: ToolName, reason: String },
    #[error("Grok stable tool name collision: `{wire_name}`")]
    WireNameCollision { wire_name: String },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GrokToolCallDecodeError {
    #[error("invalid arguments for Grok local function `{wire_name}`: {reason}")]
    InvalidFunctionArguments { wire_name: String, reason: String },
}

impl GrokToolPlan {
    pub fn decode_local_function_call(
        &self,
        wire_name: &str,
        arguments: &str,
    ) -> Result<Option<GrokDecodedLocalToolCall>, GrokToolCallDecodeError> {
        let Some(route) = self.local_routes.get(wire_name) else {
            return Ok(None);
        };
        let input = match &route.input_projection {
            GrokLocalInputProjection::Function => {
                GrokLocalToolInput::FunctionArguments(arguments.to_string())
            }
            GrokLocalInputProjection::FunctionEnvelope { input_key } => {
                let value = decode_single_wrapper_argument(wire_name, arguments, input_key)?;
                let arguments = serde_json::to_string(&value).map_err(|error| {
                    GrokToolCallDecodeError::InvalidFunctionArguments {
                        wire_name: wire_name.to_string(),
                        reason: error.to_string(),
                    }
                })?;
                GrokLocalToolInput::FunctionArguments(arguments)
            }
            GrokLocalInputProjection::Freeform { input_key } => {
                let value = decode_single_wrapper_argument(wire_name, arguments, input_key)?;
                let Some(input) = value.as_str() else {
                    return Err(GrokToolCallDecodeError::InvalidFunctionArguments {
                        wire_name: wire_name.to_string(),
                        reason: format!("expected `{input_key}` to be a string"),
                    });
                };
                GrokLocalToolInput::Freeform(input.to_string())
            }
            GrokLocalInputProjection::ToolSearch => {
                GrokLocalToolInput::ToolSearchArguments(arguments.to_string())
            }
        };
        Ok(Some(GrokDecodedLocalToolCall {
            canonical_identity: route.canonical_identity.clone(),
            input,
        }))
    }
}

pub fn plan_grok_tools(local_tools: Vec<GrokLocalTool>) -> Result<GrokToolPlan, GrokToolPlanError> {
    let mut declarations = Vec::with_capacity(local_tools.len());
    let mut local_routes = BTreeMap::new();

    for local_tool in local_tools {
        let identity = local_tool.identity;
        let (wire_name, tool, input_projection) = match local_tool.spec {
            ToolSpec::ToolSearch {
                execution,
                description,
                parameters,
            } if identity == ToolName::plain("tool_search") && execution == "client" => (
                "tool_search".to_string(),
                ResponsesApiTool {
                    name: "tool_search".to_string(),
                    description,
                    strict: false,
                    defer_loading: None,
                    parameters,
                    output_schema: None,
                },
                GrokLocalInputProjection::ToolSearch,
            ),
            ToolSpec::Function(mut tool)
                if identity.is_default_namespace() && tool.name == identity.name =>
            {
                tool.defer_loading = None;
                let wire_name = if is_safe_ordinary_wire_name(&identity.name) {
                    identity.name.clone()
                } else {
                    derived_wire_name("function", &identity)
                };
                let (tool, input_projection) = project_function_tool(tool);
                (wire_name, tool, input_projection)
            }
            ToolSpec::Freeform(tool)
                if identity.is_default_namespace() && tool.name == identity.name =>
            {
                let wire_name = if is_safe_ordinary_wire_name(&identity.name) {
                    identity.name.clone()
                } else {
                    derived_wire_name("freeform", &identity)
                };
                let (tool, input_projection) = project_freeform_tool(tool).map_err(|reason| {
                    GrokToolPlanError::UnsupportedLocalTool {
                        identity: identity.clone(),
                        reason,
                    }
                })?;
                (wire_name, tool, input_projection)
            }
            ToolSpec::Namespace(namespace) => {
                if identity.namespace.as_deref() != Some(namespace.name.as_str()) {
                    return Err(GrokToolPlanError::UnsupportedLocalTool {
                        identity,
                        reason: "canonical namespace does not match the tool declaration"
                            .to_string(),
                    });
                }
                let Some(namespace_tool) = namespace.tools.into_iter().find(|tool| match tool {
                    ResponsesApiNamespaceTool::Function(tool) => tool.name == identity.name,
                    ResponsesApiNamespaceTool::Custom(tool) => tool.name == identity.name,
                }) else {
                    return Err(GrokToolPlanError::UnsupportedLocalTool {
                        identity,
                        reason: "canonical child is absent from the Namespace declaration"
                            .to_string(),
                    });
                };
                match namespace_tool {
                    ResponsesApiNamespaceTool::Function(tool) => {
                        let (tool, input_projection) = project_function_tool(tool);
                        (
                            derived_wire_name("function", &identity),
                            tool,
                            input_projection,
                        )
                    }
                    ResponsesApiNamespaceTool::Custom(tool) => {
                        let (tool, input_projection) =
                            project_freeform_tool(tool).map_err(|reason| {
                                GrokToolPlanError::UnsupportedLocalTool {
                                    identity: identity.clone(),
                                    reason,
                                }
                            })?;
                        (
                            derived_wire_name("freeform", &identity),
                            tool,
                            input_projection,
                        )
                    }
                }
            }
            _ => {
                return Err(GrokToolPlanError::UnsupportedLocalTool {
                    identity,
                    reason: "unsupported local tool declaration".to_string(),
                });
            }
        };
        let route = GrokLocalToolRoute {
            canonical_identity: identity.clone(),
            input_projection,
        };
        if local_routes.insert(wire_name.clone(), route).is_some() {
            return Err(GrokToolPlanError::WireNameCollision { wire_name });
        }
        declarations.push(ToolSpec::Function(ResponsesApiTool {
            name: wire_name,
            description: tool.description,
            strict: tool.strict,
            defer_loading: None,
            parameters: tool.parameters,
            output_schema: tool.output_schema,
        }));
    }

    Ok(GrokToolPlan {
        declarations,
        local_routes,
    })
}

fn project_function_tool(
    mut tool: ResponsesApiTool,
) -> (ResponsesApiTool, GrokLocalInputProjection) {
    if schema_root_is_object_only(&tool.parameters) {
        return (tool, GrokLocalInputProjection::Function);
    }

    let input_key = "input".to_string();
    let mut input_schema = tool.parameters;
    let defs = input_schema.defs.take();
    let definitions = input_schema.definitions.take();
    let mut parameters = JsonSchema::object(
        BTreeMap::from([(input_key.clone(), input_schema)]),
        Some(vec![input_key.clone()]),
        Some(false.into()),
    );
    parameters.defs = defs;
    parameters.definitions = definitions;
    tool.parameters = parameters;
    (
        tool,
        GrokLocalInputProjection::FunctionEnvelope { input_key },
    )
}

fn schema_root_is_object_only(schema: &JsonSchema) -> bool {
    let any_of_is_object_only = schema
        .any_of
        .as_ref()
        .map(|variants| !variants.is_empty() && variants.iter().all(schema_root_is_object_only));
    let one_of_is_object_only = schema
        .one_of
        .as_ref()
        .map(|variants| !variants.is_empty() && variants.iter().all(schema_root_is_object_only));
    if matches!(any_of_is_object_only, Some(false)) || matches!(one_of_is_object_only, Some(false))
    {
        return false;
    }

    match &schema.schema_type {
        Some(JsonSchemaType::Single(schema_type)) => {
            return *schema_type == JsonSchemaPrimitiveType::Object;
        }
        Some(JsonSchemaType::Multiple(schema_types)) => {
            return !schema_types.is_empty()
                && schema_types
                    .iter()
                    .all(|schema_type| *schema_type == JsonSchemaPrimitiveType::Object);
        }
        None => {}
    }

    any_of_is_object_only == Some(true)
        || one_of_is_object_only == Some(true)
        || schema
            .all_of
            .as_ref()
            .is_some_and(|variants| variants.iter().any(schema_root_is_object_only))
}

fn decode_single_wrapper_argument(
    wire_name: &str,
    arguments: &str,
    input_key: &str,
) -> Result<Value, GrokToolCallDecodeError> {
    let value: Value = serde_json::from_str(arguments).map_err(|error| {
        GrokToolCallDecodeError::InvalidFunctionArguments {
            wire_name: wire_name.to_string(),
            reason: error.to_string(),
        }
    })?;
    let Some(object) = value.as_object() else {
        return Err(GrokToolCallDecodeError::InvalidFunctionArguments {
            wire_name: wire_name.to_string(),
            reason: "expected one JSON object argument".to_string(),
        });
    };
    if object.len() != 1 {
        return Err(GrokToolCallDecodeError::InvalidFunctionArguments {
            wire_name: wire_name.to_string(),
            reason: format!("expected only the `{input_key}` field"),
        });
    }
    object.get(input_key).cloned().ok_or_else(|| {
        GrokToolCallDecodeError::InvalidFunctionArguments {
            wire_name: wire_name.to_string(),
            reason: format!("expected the `{input_key}` field"),
        }
    })
}

fn project_freeform_tool(
    tool: FreeformTool,
) -> Result<(ResponsesApiTool, GrokLocalInputProjection), String> {
    let FreeformTool {
        name,
        description,
        format,
        ..
    } = tool;
    let (input_key, input_description) = match name.as_str() {
        "apply_patch" => ("patch", "Patch text passed unchanged to Local Codex."),
        "exec" => (
            "source",
            "JavaScript source passed unchanged to Local Codex.",
        ),
        _ => ("input", "Freeform input passed unchanged to Local Codex."),
    };
    let parameters = JsonSchema::object(
        BTreeMap::from([(
            input_key.to_string(),
            JsonSchema::string(Some(input_description.to_string())),
        )]),
        Some(vec![input_key.to_string()]),
        Some(false.into()),
    );
    let format = serde_json::to_string(&format)
        .map_err(|error| format!("freeform grammar metadata is not representable: {error}"))?;
    let description = format!("{description}\n\n{FREEFORM_FORMAT_DESCRIPTION_HEADER}\n{format}");
    Ok((
        ResponsesApiTool {
            name,
            description,
            strict: true,
            defer_loading: None,
            parameters,
            output_schema: None,
        },
        GrokLocalInputProjection::Freeform {
            input_key: input_key.to_string(),
        },
    ))
}

fn derived_wire_name(kind: &str, identity: &ToolName) -> String {
    let namespace = identity.namespace.as_deref().unwrap_or_default();
    let tuple = [HASH_VERSION, kind, namespace, identity.name.as_str()]
        .map(|field| format!("{}:{field}", field.len()))
        .join("|");
    let digest = format!("{:x}", Sha256::digest(tuple.as_bytes()));
    let slug = format!("{namespace}_{}", identity.name)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let max_slug_len = MAX_WIRE_NAME_LEN - DERIVED_NAME_PREFIX.len() - "__".len() - HASH_SUFFIX_LEN;
    let slug = slug.trim_start_matches('_');
    let slug = &slug[..slug.len().min(max_slug_len)];
    format!(
        "{DERIVED_NAME_PREFIX}{slug}__{}",
        &digest[..HASH_SUFFIX_LEN]
    )
}

fn is_safe_ordinary_wire_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_WIRE_NAME_LEN
        && !name.starts_with(DERIVED_NAME_PREFIX)
        && !RESERVED_WIRE_NAMES.contains(&name)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
