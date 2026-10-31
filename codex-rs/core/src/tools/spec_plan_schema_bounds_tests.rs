use super::*;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::MultiAgentToolMessages;
use codex_protocol::openai_models::ToolMessage;
use codex_protocol::openai_models::ToolMessages;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[tokio::test]
async fn catalog_bounds_survive_actual_planning_with_encryption_and_invalid_fallback() {
    let parameters = json!({
        "type":"object", "properties": {
            "message":{"type":"string"},
            "task_name":{"type":"string"},
            "fork_turns":{"type":["string","integer"],"minimum":1,"maximum":3}
        }, "required":["message","task_name"], "additionalProperties":false
    });
    let bundled = probe(|turn| set_feature(turn, Feature::MultiAgentV2, /*enabled*/ true)).await;
    let bundled = serde_json::to_value(bundled.visible_spec(MULTI_AGENT_V2_NAMESPACE)).unwrap();
    let bundled_spawn = bundled["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "spawn_agent")
        .unwrap();
    for invalid in [false, true] {
        let mut input = parameters.clone();
        if invalid {
            input["properties"]["fork_turns"]["maximum"] = json!("three");
        }
        let plan = probe(|turn| {
            set_feature(turn, Feature::MultiAgentV2, /*enabled*/ true);
            update_turn_settings_for_test(turn, |settings| {
                Arc::make_mut(&mut settings.model_info).model_messages = Some(ModelMessages {
                    tools: Some(ToolMessages {
                        multi_agent: Some(MultiAgentToolMessages {
                            spawn_agent: Some(ToolMessage {
                                description: None,
                                parameters: Some(input.to_string()),
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            });
        })
        .await;
        let spec = serde_json::to_value(plan.visible_spec(MULTI_AGENT_V2_NAMESPACE)).unwrap();
        let actual = &spec["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "spawn_agent")
            .unwrap()["parameters"];
        let mut expected = if invalid {
            bundled_spawn["parameters"].clone()
        } else {
            parameters.clone()
        };
        expected["properties"]["message"]["encrypted"] = json!(true);
        assert_eq!(actual, &expected);
    }
}

#[test]
fn catalog_wait_bounds_round_trip_and_malformed_bound_falls_back() {
    use crate::tools::code_mode::wait_spec::create_wait_tool;
    let parameters = json!({"type":"object","properties":{
        "cell_id":{"type":"string"},
        "yield_time_ms":{"type":"number","minimum":0.5,"maximum":9007199254740993_u64}
    }, "required":["cell_id"], "additionalProperties":false});
    let tool = create_wait_tool(
        /*description_override*/ None,
        Some(&parameters.to_string()),
    );
    assert_eq!(
        serde_json::to_value(tool).unwrap()["parameters"],
        parameters
    );
    for (field, invalid) in [
        ("minimum", json!(false)),
        ("maximum", json!({})),
        ("maxItems", json!(-1)),
    ] {
        let mut bad = parameters.clone();
        bad["properties"]["yield_time_ms"][field] = invalid;
        assert_eq!(
            create_wait_tool(/*description_override*/ None, Some(&bad.to_string())),
            create_wait_tool(
                /*description_override*/ None, /*parameters_override*/ None
            )
        );
    }
}

#[tokio::test]
async fn bounded_dynamic_schema_reaches_direct_plan_and_code_mode_registry() {
    let schema = json!({"type":"object","properties":{
        "images":{"type":"array","items":{"type":"string"},"maxItems":3},
        "count":{"type":"integer","minimum":1,"maximum":3}
    },"additionalProperties":false});
    for code_mode in [false, true] {
        let function = codex_protocol::dynamic_tools::DynamicToolFunctionSpec {
            name: "bounded_input".to_string(),
            description: "A bounded input".to_string(),
            input_schema: schema.clone(),
            defer_loading: false,
        };
        let plan = probe_with(
            |turn| {
                if code_mode {
                    set_features(turn, &[Feature::CodeMode, Feature::CodeModeOnly]);
                }
            },
            ToolPlanInputs {
                dynamic_tools: vec![DynamicToolSpec::Function(function)],
                ..Default::default()
            },
        )
        .await;
        if code_mode {
            assert_eq!(
                plan.code_mode_tool_names.get("bounded_input"),
                Some(&ToolName::plain("bounded_input"))
            );
            let spec =
                serde_json::to_value(plan.visible_spec(codex_code_mode::PUBLIC_TOOL_NAME)).unwrap();
            assert!(
                spec["description"]
                    .as_str()
                    .unwrap()
                    .contains("bounded_input")
            );
        } else {
            let spec: Value = serde_json::to_value(plan.visible_spec("bounded_input")).unwrap();
            assert_eq!(spec["parameters"], schema);
        }
    }
}
