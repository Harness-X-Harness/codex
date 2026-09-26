use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const STRUCTURED_EDIT_TOOL_NAME: &str = "structured_edit";

pub fn create_structured_edit_tool(include_environment_id: bool) -> ToolSpec {
    let mut properties = BTreeMap::from([
        (
            "file_path".to_string(),
            JsonSchema::string(Some(
                "Path of an existing text file to edit, relative to the environment cwd unless absolute."
                    .to_string(),
            )),
        ),
        (
            "old_string".to_string(),
            JsonSchema::string(Some(
                "Exact UTF-8 text to find. Must be non-empty and must differ from new_string."
                    .to_string(),
            )),
        ),
        (
            "new_string".to_string(),
            JsonSchema::string(Some(
                "Exact UTF-8 replacement for old_string. Unmatched file bytes are preserved."
                    .to_string(),
            )),
        ),
        (
            "replace_all".to_string(),
            JsonSchema::boolean(Some(
                "When true, replace every exact match. When false or omitted, require exactly one match."
                    .to_string(),
            )),
        ),
    ]);
    if include_environment_id {
        properties.insert(
            "environment_id".to_string(),
            JsonSchema::string(Some(
                "Environment id from <environment_context>. Omit to use the primary environment."
                    .to_string(),
            )),
        );
    }

    ToolSpec::Function(ResponsesApiTool {
        name: STRUCTURED_EDIT_TOOL_NAME.to_string(),
        description: "Edit an existing text file by replacing exact UTF-8 text. Use this instead of generating a patch. The file must already exist; this tool cannot create or overwrite a file from empty input. Zero matches, or multiple matches when replace_all is false, fail without changing the file.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec![
                "file_path".to_string(),
                "old_string".to_string(),
                "new_string".to_string(),
            ]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}
