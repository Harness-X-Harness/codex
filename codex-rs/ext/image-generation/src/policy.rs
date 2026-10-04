use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use codex_api::ApiDialect;
use codex_api::GROK_IMAGE_MODEL;
use codex_api::GROK_MAX_EDIT_IMAGES;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::default_namespace_description;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_image::normalize_to_png;
use schemars::JsonSchema;
use schemars::r#gen::SchemaSettings;
use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;

use crate::IMAGE_GEN_NAMESPACE;
use crate::IMAGEGEN_TOOL_NAME;

pub(crate) const MAX_GENERATED_IMAGE_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_GENERATED_IMAGE_BASE64_BYTES: usize =
    MAX_GENERATED_IMAGE_BYTES.div_ceil(3) * 4;
const IMAGEGEN_DESCRIPTION: &str = include_str!("../imagegen_description.md");
const GROK_IMAGEGEN_DESCRIPTION: &str = include_str!("../grok_imagegen_description.md");

#[derive(Clone, Copy)]
pub(crate) struct ImagePolicy {
    pub(crate) dialect: ApiDialect,
    pub(crate) model: &'static str,
    pub(crate) max_edit_images: usize,
}

impl ImagePolicy {
    pub(crate) fn for_dialect(dialect: ApiDialect) -> Self {
        match dialect {
            ApiDialect::OpenAi => Self {
                dialect,
                model: "gpt-image-2",
                max_edit_images: 5,
            },
            ApiDialect::Grok => Self {
                dialect,
                model: GROK_IMAGE_MODEL,
                max_edit_images: GROK_MAX_EDIT_IMAGES,
            },
        }
    }

    pub(crate) fn parse_args(self, arguments: &str) -> Result<ImagegenArgs, FunctionCallError> {
        if self.dialect == ApiDialect::Grok {
            let value: Value = serde_json::from_str(arguments)
                .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
            let Value::Object(value) = value else {
                return Err(FunctionCallError::RespondToModel(
                    "Grok image tool arguments must be an object".to_string(),
                ));
            };
            if value.contains_key("transparent_background") {
                return Err(FunctionCallError::RespondToModel(
                    "transparent_background is not supported by the Grok image tool".to_string(),
                ));
            }
        }
        serde_json::from_str(arguments)
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
    }

    pub(crate) fn normalize_result(self, result: String) -> Result<String, String> {
        if self.dialect == ApiDialect::OpenAi {
            return Ok(result);
        }
        if result.len() > MAX_GENERATED_IMAGE_BASE64_BYTES {
            return Err("generated image exceeds the local image byte limit".to_string());
        }
        let bytes = BASE64_STANDARD
            .decode(result)
            .map_err(|_| "generated image is not valid base64".to_string())?;
        let image = normalize_to_png(
            Path::new("generated-image"),
            bytes,
            MAX_GENERATED_IMAGE_BYTES,
        )
        .map_err(|error| format!("unable to normalize generated image: {error}"))?;
        Ok(BASE64_STANDARD.encode(image.bytes))
    }

    /// Builds the namespace function schema exposed to the model.
    pub(crate) fn tool_spec(self) -> ToolSpec {
        let mut schema_value = serde_json::to_value(
            SchemaSettings::draft2019_09()
                .with(|settings| settings.inline_subschemas = true)
                .into_generator()
                .into_root_schema_for::<ImagegenArgs>(),
        )
        .unwrap_or_else(|err| panic!("imagegen schema should serialize: {err}"));
        let Value::Object(ref mut schema) = schema_value else {
            unreachable!("imagegen root schema must be an object");
        };
        let mut input_schema = Map::new();
        for key in ["properties", "required", "type", "additionalProperties"] {
            if let Some(value) = schema.remove(key) {
                input_schema.insert(key.to_string(), value);
            }
        }
        if self.dialect == ApiDialect::Grok {
            let properties = input_schema
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .unwrap_or_else(|| unreachable!("image argument properties"));
            properties.remove("transparent_background");
            properties
                .get_mut("referenced_image_paths")
                .unwrap_or_else(|| unreachable!("path argument schema"))["maxItems"] =
                self.max_edit_images.into();
            properties
                .get_mut("num_last_images_to_include")
                .unwrap_or_else(|| unreachable!("history argument schema"))["maximum"] =
                self.max_edit_images.into();
        }
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: IMAGE_GEN_NAMESPACE.to_string(),
            description: default_namespace_description(IMAGE_GEN_NAMESPACE),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: IMAGEGEN_TOOL_NAME.to_string(),
                description: match self.dialect {
                    ApiDialect::OpenAi => IMAGEGEN_DESCRIPTION,
                    ApiDialect::Grok => GROK_IMAGEGEN_DESCRIPTION,
                }
                .to_string(),
                strict: false,
                parameters: parse_tool_input_schema(&Value::Object(input_schema))
                    .unwrap_or_else(|err| panic!("imagegen input schema should parse: {err}")),
                output_schema: None,
                defer_loading: None,
            })],
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImagegenArgs {
    pub(crate) prompt: String,
    /// Whether the output should have a transparent background. Defaults to false.
    #[serde(default)]
    pub(crate) transparent_background: bool,
    #[schemars(length(max = 5))]
    pub(crate) referenced_image_paths: Option<Vec<AbsolutePathBuf>>,
    #[schemars(range(min = 1, max = 5))]
    pub(crate) num_last_images_to_include: Option<usize>,
}
