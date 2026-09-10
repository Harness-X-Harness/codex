//! Grok-owned tests for flat-projected `apply_patch` grammar safety.
//!
//! Grok advertises `apply_patch` as a JSON-schema function with a `patch`
//! string. Reverse projection must not treat a string-typed field as
//! grammar-valid `custom_tool_call.input`.

use crate::function_tool::FunctionCallError;
use crate::tools::handlers::apply_patch_spec::create_apply_patch_freeform_tool;
use crate::tools::registry::ToolRegistry;
use crate::tools::router::ToolRouter;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ToolMode;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

const CANONICAL_PATCH: &str = "\
*** Begin Patch
*** Add File: foo.txt
+hi
*** End Patch
";

const DECORATED_BEGIN_PATCH: &str = "\
*** Begin Patch ***
*** Add File: foo.txt
+hi
*** End Patch
";

const DECORATED_END_PATCH: &str = "\
*** Begin Patch
*** Add File: foo.txt
+hi
*** End Patch ***
";

const DECORATED_END_OF_FILE_AFTER_TERMINATOR: &str = "\
*** Begin Patch
*** Add File: foo.txt
+hi
*** End Patch ***
*** End of File ***
";

const MALFORMED_INTERNAL_HUNK: &str = "\
*** Begin Patch
*** Update File: foo.txt
THIS IS NOT A VALID HUNK
*** End Patch
";

fn grok_apply_patch_router() -> ToolRouter {
    ToolRouter::from_parts_with_projection(
        ToolRegistry::default(),
        vec![create_apply_patch_freeform_tool(false)],
        ToolMode::Direct,
        BTreeMap::new(),
        None,
        &[],
        true,
    )
    .expect("flat apply_patch projection should compile")
}

fn restore_patch(router: &ToolRouter, patch: &str) -> Result<ResponseItem, FunctionCallError> {
    let declared_name = match &router.model_visible_specs()[0] {
        ToolSpec::Function(tool) => tool.name.clone(),
        spec => panic!("expected projected function, got {spec:?}"),
    };
    let mut item = ResponseItem::FunctionCall {
        id: None,
        name: declared_name,
        namespace: None,
        arguments: json!({ "patch": patch }).to_string(),
        encrypted_function_args: None,
        call_id: "call-grok-apply-patch".to_string(),
        internal_chat_message_metadata_passthrough: None,
    };
    router.restore_tool_call(&mut item)?;
    Ok(item)
}

#[test]
fn grok_restore_accepts_canonical_apply_patch_markers() {
    let router = grok_apply_patch_router();
    let item = restore_patch(&router, CANONICAL_PATCH).expect("canonical patch should restore");
    assert_eq!(
        item,
        ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-grok-apply-patch".to_string(),
            name: "apply_patch".to_string(),
            namespace: None,
            input: CANONICAL_PATCH.to_string(),
            internal_chat_message_metadata_passthrough: None,
        }
    );
}

#[test]
fn grok_restore_rejects_decorated_begin_patch_marker() {
    let router = grok_apply_patch_router();
    let error = restore_patch(&router, DECORATED_BEGIN_PATCH)
        .expect_err("decorated begin marker must fail at the Provider boundary");
    assert_eq!(
        error,
        FunctionCallError::RespondToModel(
            "apply_patch grammar rejected at the Provider boundary: expected first line \"*** Begin Patch\", got \"*** Begin Patch ***\"".to_string()
        )
    );
}

#[test]
fn grok_restore_rejects_decorated_end_patch_marker() {
    let router = grok_apply_patch_router();
    let error = restore_patch(&router, DECORATED_END_PATCH)
        .expect_err("decorated end marker must fail at the Provider boundary");
    assert_eq!(
        error,
        FunctionCallError::RespondToModel(
            "apply_patch grammar rejected at the Provider boundary: expected last line \"*** End Patch\", got \"*** End Patch ***\"".to_string()
        )
    );
}

#[test]
fn grok_restore_rejects_end_of_file_after_patch_terminator() {
    let router = grok_apply_patch_router();
    let error = restore_patch(&router, DECORATED_END_OF_FILE_AFTER_TERMINATOR)
        .expect_err("text after EndPatch must fail at the Provider boundary");
    assert_eq!(
        error,
        FunctionCallError::RespondToModel(
            "apply_patch grammar rejected at the Provider boundary: expected last line \"*** End Patch\", got \"*** End of File ***\"".to_string()
        )
    );
}

#[test]
fn grok_restore_rejects_malformed_internal_patch_grammar() {
    let router = grok_apply_patch_router();
    let error = restore_patch(&router, MALFORMED_INTERNAL_HUNK)
        .expect_err("invalid internal grammar must fail at the Provider boundary");
    let FunctionCallError::RespondToModel(message) = error else {
        panic!("malformed projected patch should produce a model-visible Provider-boundary error");
    };
    assert!(message.starts_with("apply_patch grammar rejected at the Provider boundary:"));
    assert!(!message.contains("expected first line"));
    assert!(!message.contains("expected last line"));
}
