use super::super::structured_edit_spec::STRUCTURED_EDIT_TOOL_NAME;
use super::super::structured_edit_spec::create_structured_edit_tool;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::structured_edit::StructuredEditHandler;
use crate::tools::registry::ToolExecutor;
use codex_tools::JsonSchema;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct StructuredEditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
    #[serde(default)]
    environment_id: Option<String>,
}

#[test]
fn structured_edit_is_a_closed_function_tool() {
    let ToolSpec::Function(tool) =
        create_structured_edit_tool(/*include_environment_id*/ false)
    else {
        panic!("structured_edit must be a function tool");
    };
    assert_eq!(tool.name, STRUCTURED_EDIT_TOOL_NAME);
    assert!(!tool.strict);
    let JsonSchema {
        additional_properties: Some(additional_properties),
        required: Some(required),
        properties: Some(properties),
        ..
    } = tool.parameters
    else {
        panic!("structured_edit must advertise a closed object schema");
    };
    assert_eq!(additional_properties, false.into());
    assert_eq!(
        required,
        vec![
            "file_path".to_string(),
            "old_string".to_string(),
            "new_string".to_string()
        ]
    );
    for name in ["file_path", "old_string", "new_string", "replace_all"] {
        assert!(properties.contains_key(name), "missing {name}");
    }
    assert!(!properties.contains_key("environment_id"));
}

#[test]
fn structured_edit_includes_environment_id_only_for_multiple_environments() {
    let ToolSpec::Function(tool) =
        create_structured_edit_tool(/*include_environment_id*/ true)
    else {
        panic!("structured_edit must be a function tool");
    };
    let properties = tool.parameters.properties.expect("properties");
    assert!(properties.contains_key("environment_id"));
}

#[test]
fn structured_edit_handler_advertises_the_function_name() {
    let handler = StructuredEditHandler::new(/*multi_environment*/ false);
    assert_eq!(handler.tool_name().name, STRUCTURED_EDIT_TOOL_NAME);
}

#[test]
fn structured_edit_arguments_default_replace_all_to_false() {
    let parsed: StructuredEditArgs = parse_arguments(
        &json!({
            "file_path": "hello.txt",
            "old_string": "HELLO",
            "new_string": "WORLD"
        })
        .to_string(),
    )
    .expect("parse");
    assert_eq!(
        parsed,
        StructuredEditArgs {
            file_path: "hello.txt".to_string(),
            old_string: "HELLO".to_string(),
            new_string: "WORLD".to_string(),
            replace_all: false,
            environment_id: None,
        }
    );
}
