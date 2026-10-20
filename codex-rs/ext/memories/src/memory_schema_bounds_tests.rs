use codex_extension_api::ExtensionData;
use codex_extension_api::ToolContributor;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use crate::extension::MemoriesExtension;
use crate::extension::MemoriesExtensionConfig;

#[test]
fn generated_memory_tool_specs_keep_declared_numeric_bounds() {
    let thread = ExtensionData::new("thread");
    thread.insert(MemoriesExtensionConfig {
        version: codex_protocol::MemoryVersion::V1,
        enabled: true,
        dedicated_tools: true,
        codex_home: test_path_buf("/tmp/memory-schema-fixture").abs(),
    });
    let tools = MemoriesExtension::default().tools(&ExtensionData::new("session"), &thread);
    for (name, field, minimum) in [
        ("read", "line_offset", 1.0),
        ("read", "max_lines", 1.0),
        ("list", "max_results", 1.0),
        ("search", "context_lines", 0.0),
        ("search", "max_results", 1.0),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool.tool_name().name == name)
            .expect("memory tool");
        let spec = serde_json::to_value(tool.spec()).expect("actual tool spec");
        assert_eq!(
            spec["tools"][0]["parameters"]["properties"][field]["minimum"].as_f64(),
            Some(minimum)
        );
    }
}
