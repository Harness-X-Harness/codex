//! Grok-owned Tool Plan coverage for the release-bundled catalog shape:
//! UnifiedExec plus advertised free-form `apply_patch`.

use super::*;

#[tokio::test]
async fn grok_catalog_shape_exposes_apply_patch_with_unified_exec() {
    let plan = probe(|turn| {
        update_turn_settings_for_test(turn, |settings| {
            let model = Arc::make_mut(&mut settings.model_info);
            model.shell_type = ConfigShellToolType::UnifiedExec;
            model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
        });
    })
    .await;
    plan.assert_visible_contains(&["apply_patch", "exec_command", "write_stdin"]);
    assert!(matches!(
        plan.visible_spec("apply_patch"),
        ToolSpec::Freeform(tool) if tool.name == "apply_patch"
    ));
}

#[tokio::test]
async fn grok_catalog_shape_keeps_apply_patch_when_shell_tool_is_disabled() {
    let plan = probe(|turn| {
        set_feature(turn, Feature::ShellTool, /*enabled*/ false);
        update_turn_settings_for_test(turn, |settings| {
            let model = Arc::make_mut(&mut settings.model_info);
            model.shell_type = ConfigShellToolType::UnifiedExec;
            model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
        });
    })
    .await;
    plan.assert_visible_contains(&["apply_patch"]);
    plan.assert_visible_lacks(&["exec_command", "write_stdin"]);
}
