use super::*;
use codex_protocol::openai_models::StructuredEditToolType;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn structured_edit_registration_follows_capability_and_environment() {
    for (capability, environment, stock) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
        (true, true, true),
    ] {
        let plan = probe(|turn| {
            if !environment {
                turn.initial_environments.environments.clear();
            }
            update_turn_settings_for_test(turn, |settings| {
                let model = Arc::make_mut(&mut settings.model_info);
                model.structured_edit_tool_type =
                    capability.then_some(StructuredEditToolType::ExactMatch);
                model.apply_patch_tool_type = stock.then_some(ApplyPatchToolType::Freeform);
            });
        })
        .await;
        if capability && environment {
            plan.assert_visible_contains(&["structured_edit"]);
            plan.assert_registered_contains(&["structured_edit"]);
            assert!(!has_parameter(
                plan.visible_spec("structured_edit"),
                "environment_id"
            ));
        } else {
            plan.assert_visible_lacks(&["structured_edit"]);
            plan.assert_registered_lacks(&["structured_edit"]);
        }
        if stock && environment {
            plan.assert_registered_contains(&["apply_patch"]);
        } else {
            plan.assert_registered_lacks(&["apply_patch"]);
        }
    }
}

#[tokio::test]
async fn structured_edit_multiple_environments_and_code_mode_use_actual_registry() {
    let multiple = probe(|turn| {
        duplicate_primary_environment(turn);
        update_turn_settings_for_test(turn, |settings| {
            Arc::make_mut(&mut settings.model_info).structured_edit_tool_type =
                Some(StructuredEditToolType::ExactMatch);
        });
    })
    .await;
    assert!(has_parameter(
        multiple.visible_spec("structured_edit"),
        "environment_id"
    ));
    let code_mode = probe(|turn| {
        set_features(turn, &[Feature::CodeMode, Feature::CodeModeOnly]);
        update_turn_settings_for_test(turn, |settings| {
            Arc::make_mut(&mut settings.model_info).structured_edit_tool_type =
                Some(StructuredEditToolType::ExactMatch);
        });
    })
    .await;
    code_mode.assert_registered_contains(&["structured_edit", "exec"]);
    code_mode.assert_visible_lacks(&["structured_edit"]);
    assert_eq!(
        code_mode.code_mode_tool_names.get("structured_edit"),
        Some(&ToolName::plain("structured_edit"))
    );
    assert!(code_mode.requires_code_mode_worker);
}
