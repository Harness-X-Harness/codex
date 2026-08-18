use crate::FreeformTool;
use crate::FreeformToolFormat;
use crate::GrokLocalInputProjection;
use crate::GrokLocalTool;
use crate::GrokLocalToolInput;
use crate::GrokLocalToolRoute;
use crate::GrokToolCallDecodeError;
use crate::GrokToolPlan;
use crate::GrokToolPlanError;
use crate::JsonSchema;
use crate::ResponsesApiNamespace;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use crate::ToolName;
use crate::ToolSpec;
use crate::plan_grok_tools;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

#[test]
fn namespace_child_plans_as_stable_local_function() {
    let parameters = JsonSchema::object(BTreeMap::new(), Some(Vec::new()), Some(false.into()));
    let canonical_tool = GrokLocalTool {
        identity: ToolName::namespaced("alpha", "search"),
        spec: ToolSpec::Namespace(ResponsesApiNamespace {
            name: "alpha".to_string(),
            description: "Alpha tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: "search".to_string(),
                description: "Search alpha.".to_string(),
                strict: true,
                defer_loading: None,
                parameters: parameters.clone(),
                output_schema: None,
            })],
        }),
    };

    let plan = plan_grok_tools(vec![canonical_tool]).expect("namespace child should be plannable");

    assert_eq!(
        plan,
        GrokToolPlan {
            declarations: vec![ToolSpec::Function(ResponsesApiTool {
                name: "local__alpha_search__27de2572f1d5fb99".to_string(),
                description: "Search alpha.".to_string(),
                strict: true,
                defer_loading: None,
                parameters,
                output_schema: None,
            })],
            local_routes: BTreeMap::from([(
                "local__alpha_search__27de2572f1d5fb99".to_string(),
                GrokLocalToolRoute {
                    canonical_identity: ToolName::namespaced("alpha", "search"),
                    input_projection: GrokLocalInputProjection::Function,
                },
            )]),
        }
    );
}

#[test]
fn safe_plain_function_keeps_its_wire_name() {
    let parameters = JsonSchema::object(BTreeMap::new(), Some(Vec::new()), Some(false.into()));
    let function = ResponsesApiTool {
        name: "shell_command".to_string(),
        description: "Run a local shell command.".to_string(),
        strict: true,
        defer_loading: None,
        parameters,
        output_schema: None,
    };

    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("shell_command"),
        spec: ToolSpec::Function(function.clone()),
    }])
    .expect("plain function should be plannable");

    assert_eq!(
        plan,
        GrokToolPlan {
            declarations: vec![ToolSpec::Function(function)],
            local_routes: BTreeMap::from([(
                "shell_command".to_string(),
                GrokLocalToolRoute {
                    canonical_identity: ToolName::plain("shell_command"),
                    input_projection: GrokLocalInputProjection::Function,
                },
            )]),
        }
    );
}

#[test]
fn client_tool_search_projects_to_ordinary_function_and_decodes_canonically() {
    let parameters = JsonSchema::object(
        BTreeMap::from([(
            "query".to_string(),
            JsonSchema::string(Some("Search query.".to_string())),
        )]),
        Some(vec!["query".to_string()]),
        Some(false.into()),
    );
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("tool_search"),
        spec: ToolSpec::ToolSearch {
            execution: "client".to_string(),
            description: "Search deferred tools.".to_string(),
            parameters: parameters.clone(),
        },
    }])
    .expect("client tool_search should project to an ordinary Grok function");

    assert_eq!(
        plan.declarations,
        vec![ToolSpec::Function(ResponsesApiTool {
            name: "tool_search".to_string(),
            description: "Search deferred tools.".to_string(),
            strict: false,
            defer_loading: None,
            parameters,
            output_schema: None,
        })]
    );
    let arguments = r#"{"query":"calendar","limit":2}"#;
    let decoded = plan
        .decode_local_function_call("tool_search", arguments)
        .expect("tool_search arguments should decode")
        .expect("tool_search should be locally owned");
    assert_eq!(decoded.canonical_identity, ToolName::plain("tool_search"));
    assert_eq!(
        decoded.input,
        GrokLocalToolInput::ToolSearchArguments(arguments.to_string())
    );
}

#[test]
fn namespaced_union_root_function_round_trips_through_object_envelope() {
    let original_parameters = JsonSchema::any_of(
        vec![
            JsonSchema::object(
                BTreeMap::from([(
                    "mode".to_string(),
                    JsonSchema::string(Some("Automation update mode.".to_string())),
                )]),
                Some(vec!["mode".to_string()]),
                Some(false.into()),
            ),
            JsonSchema::null(None),
        ],
        Some("Update an automation, or clear the current value.".to_string()),
    );
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::namespaced("codex_app", "automation_update"),
        spec: ToolSpec::Namespace(ResponsesApiNamespace {
            name: "codex_app".to_string(),
            description: "Tools provided by the Codex app.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: "automation_update".to_string(),
                description: "Create or update automations.".to_string(),
                strict: false,
                defer_loading: Some(true),
                parameters: original_parameters.clone(),
                output_schema: None,
            })],
        }),
    }])
    .expect("union-root dynamic tool should be reversibly plannable");

    let ToolSpec::Function(function) = &plan.declarations[0] else {
        panic!("Grok local tool must be declared as a function");
    };
    assert_eq!(
        function.parameters,
        JsonSchema::object(
            BTreeMap::from([("input".to_string(), original_parameters)]),
            Some(vec!["input".to_string()]),
            Some(false.into()),
        )
    );
    assert_eq!(
        plan.local_routes[&function.name].input_projection,
        GrokLocalInputProjection::FunctionEnvelope {
            input_key: "input".to_string(),
        }
    );

    let object_input = serde_json::json!({"mode": "create"});
    let decoded = plan
        .decode_local_function_call(
            &function.name,
            &serde_json::json!({"input": object_input.clone()}).to_string(),
        )
        .expect("object branch should decode")
        .expect("dynamic tool should be locally owned");
    let GrokLocalToolInput::FunctionArguments(arguments) = decoded.input else {
        panic!("dynamic function must decode as function arguments");
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&arguments).expect("valid decoded arguments"),
        object_input
    );

    let decoded = plan
        .decode_local_function_call(&function.name, r#"{"input":null}"#)
        .expect("null branch should decode")
        .expect("dynamic tool should be locally owned");
    assert_eq!(
        decoded.input,
        GrokLocalToolInput::FunctionArguments("null".to_string())
    );
}

#[test]
fn function_envelope_keeps_local_definition_refs_resolvable() {
    let original_parameters = JsonSchema {
        one_of: Some(vec![
            JsonSchema {
                schema_ref: Some("#/$defs/CreateInput".to_string()),
                ..Default::default()
            },
            JsonSchema::null(None),
        ]),
        defs: Some(BTreeMap::from([(
            "CreateInput".to_string(),
            JsonSchema::object(
                BTreeMap::from([(
                    "name".to_string(),
                    JsonSchema::string(Some("Automation name.".to_string())),
                )]),
                Some(vec!["name".to_string()]),
                Some(false.into()),
            ),
        )])),
        definitions: Some(BTreeMap::from([(
            "LegacyInput".to_string(),
            JsonSchema::string(Some("Legacy automation identifier.".to_string())),
        )])),
        ..Default::default()
    };
    let mut expected_input = original_parameters.clone();
    let expected_defs = expected_input.defs.take();
    let expected_definitions = expected_input.definitions.take();

    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::namespaced("codex_app", "automation_update"),
        spec: ToolSpec::Namespace(ResponsesApiNamespace {
            name: "codex_app".to_string(),
            description: "Tools provided by the Codex app.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: "automation_update".to_string(),
                description: "Create or update automations.".to_string(),
                strict: false,
                defer_loading: Some(true),
                parameters: original_parameters,
                output_schema: None,
            })],
        }),
    }])
    .expect("definition-backed union schema should be reversibly plannable");

    let ToolSpec::Function(function) = &plan.declarations[0] else {
        panic!("Grok local tool must be declared as a function");
    };
    let mut expected_parameters = JsonSchema::object(
        BTreeMap::from([("input".to_string(), expected_input)]),
        Some(vec!["input".to_string()]),
        Some(false.into()),
    );
    expected_parameters.defs = expected_defs;
    expected_parameters.definitions = expected_definitions;
    assert_eq!(function.parameters, expected_parameters);

    let wire_schema = serde_json::to_value(&function.parameters).expect("serialize wire schema");
    assert_eq!(
        wire_schema.pointer("/properties/input/oneOf/0/$ref"),
        Some(&serde_json::json!("#/$defs/CreateInput"))
    );
    assert!(wire_schema.pointer("/$defs/CreateInput").is_some());
    assert!(wire_schema.pointer("/properties/input/$defs").is_none());
    assert!(wire_schema.pointer("/definitions/LegacyInput").is_some());
    assert!(
        wire_schema
            .pointer("/properties/input/definitions")
            .is_none()
    );
}

#[test]
fn malformed_function_envelope_fails_closed() {
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("union_input"),
        spec: ToolSpec::Function(ResponsesApiTool {
            name: "union_input".to_string(),
            description: "Accept a string or no value.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::any_of(
                vec![JsonSchema::string(None), JsonSchema::null(None)],
                None,
            ),
            output_schema: None,
        }),
    }])
    .expect("union-root function should be plannable");

    assert!(matches!(
        plan.decode_local_function_call("union_input", r#"{"input":"ok","unexpected":true}"#,),
        Err(GrokToolCallDecodeError::InvalidFunctionArguments { .. })
    ));
}

#[test]
fn oversized_plain_function_uses_bounded_stable_wire_name() {
    let canonical_name = "a".repeat(80);
    let function = ResponsesApiTool {
        name: canonical_name.clone(),
        description: "An oversized local tool name.".to_string(),
        strict: true,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), Some(Vec::new()), Some(false.into())),
        output_schema: None,
    };

    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain(canonical_name),
        spec: ToolSpec::Function(function),
    }])
    .expect("oversized function should use the stable derived-name profile");

    let ToolSpec::Function(planned) = &plan.declarations[0] else {
        panic!("local function must remain a function");
    };
    assert_eq!(
        planned.name,
        "local__aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa__c6ca78b775311239"
    );
    assert_eq!(planned.name.len(), 64);
    assert_eq!(
        plan.local_routes[&planned.name].canonical_identity,
        ToolName::plain("a".repeat(80))
    );
}

#[test]
fn apply_patch_freeform_round_trips_through_function_wrapper() {
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("apply_patch"),
        spec: ToolSpec::Freeform(FreeformTool {
            name: "apply_patch".to_string(),
            description: "Apply a patch locally.".to_string(),
            defer_loading: Some(true),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: PATCH".to_string(),
            },
        }),
    }])
    .expect("apply_patch freeform tool should be reversibly plannable");

    let expected_parameters = JsonSchema::object(
        BTreeMap::from([(
            "patch".to_string(),
            JsonSchema::string(Some(
                "Patch text passed unchanged to Local Codex.".to_string(),
            )),
        )]),
        Some(vec!["patch".to_string()]),
        Some(false.into()),
    );
    assert_eq!(
        plan.declarations,
        vec![ToolSpec::Function(ResponsesApiTool {
            name: "apply_patch".to_string(),
            description: concat!(
                "Apply a patch locally.\n\n",
                "Local Codex freeform grammar metadata (descriptive only; the Grok Gateway does not enforce this grammar):\n",
                r#"{"type":"grammar","syntax":"lark","definition":"start: PATCH"}"#,
            )
            .to_string(),
            strict: true,
            defer_loading: None,
            parameters: expected_parameters,
            output_schema: None,
        })]
    );
    assert_eq!(
        plan.local_routes["apply_patch"].input_projection,
        GrokLocalInputProjection::Freeform {
            input_key: "patch".to_string(),
        }
    );

    let decoded = plan
        .decode_local_function_call(
            "apply_patch",
            r#"{"patch":"*** Begin Patch\n*** End Patch"}"#,
        )
        .expect("wrapper arguments should decode")
        .expect("apply_patch is locally owned");
    assert_eq!(decoded.canonical_identity, ToolName::plain("apply_patch"));
    assert_eq!(
        decoded.input,
        GrokLocalToolInput::Freeform("*** Begin Patch\n*** End Patch".to_string())
    );
}

#[test]
fn grok_hosted_declarations_use_native_wire_types() {
    let declarations = vec![
        ToolSpec::WebSearch {
            external_web_access: None,
            indexed_web_access: None,
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        },
        ToolSpec::XSearch,
        ToolSpec::ImageGeneration,
    ];

    assert_eq!(
        serde_json::to_value(declarations).expect("hosted declarations should serialize"),
        serde_json::json!([
            {"type": "web_search"},
            {"type": "x_search"},
            {"type": "image_generation"}
        ])
    );
}

#[test]
fn stable_names_survive_repeated_plans_reordering_and_namespace_collisions() {
    let alpha = namespaced_function("alpha", "search");
    let beta = namespaced_function("beta", "search");

    let first = plan_grok_tools(vec![alpha.clone(), beta.clone()])
        .expect("both namespace children should be plannable");
    let repeated = plan_grok_tools(vec![alpha.clone(), beta.clone()])
        .expect("the repeated request should be plannable");
    let reordered =
        plan_grok_tools(vec![beta, alpha]).expect("tool order must not affect stable identities");

    let alpha_identity = ToolName::namespaced("alpha", "search");
    let beta_identity = ToolName::namespaced("beta", "search");
    let alpha_wire = wire_name_for(&first, &alpha_identity);
    let beta_wire = wire_name_for(&first, &beta_identity);

    assert_eq!(alpha_wire, wire_name_for(&repeated, &alpha_identity));
    assert_eq!(alpha_wire, wire_name_for(&reordered, &alpha_identity));
    assert_eq!(beta_wire, wire_name_for(&reordered, &beta_identity));
    assert_ne!(alpha_wire, beta_wire);
    for wire_name in [alpha_wire, beta_wire] {
        assert!(wire_name.len() <= 64);
        assert!(
            wire_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        );
    }
    assert!(first.declarations.iter().all(|tool| {
        matches!(tool, ToolSpec::Function(function) if function.defer_loading.is_none())
    }));
}

#[test]
fn hosted_name_collision_keeps_local_capability_under_stable_virtual_namespace() {
    let local_web_search = ResponsesApiTool {
        name: "web_search".to_string(),
        description: "A local tool that happens to use a hosted name.".to_string(),
        strict: true,
        defer_loading: Some(true),
        parameters: JsonSchema::object(BTreeMap::new(), Some(Vec::new()), Some(false.into())),
        output_schema: None,
    };

    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("web_search"),
        spec: ToolSpec::Function(local_web_search),
    }])
    .expect("hosted/local name collision should preserve the local tool");

    let ToolSpec::Function(function) = &plan.declarations[0] else {
        panic!("local collision must remain a function");
    };
    assert_ne!(function.name, "web_search");
    assert!(function.name.starts_with("local__web_search__"));
    assert_eq!(
        plan.local_routes[&function.name].canonical_identity,
        ToolName::plain("web_search")
    );
}

#[test]
fn exec_and_generic_freeform_round_trip_exact_string_inputs() {
    let plan = plan_grok_tools(vec![
        freeform_tool("exec"),
        freeform_tool("extension_freeform"),
    ])
    .expect("string freeform tools should be reversibly plannable");
    let source = "await tools.run({ text: \"\u{96ea}\\nquoted\" });\n";
    let generic_input = "line one\nline two: \\\"quoted\\\" \u{96ea}";

    let exec = plan
        .decode_local_function_call("exec", &serde_json::json!({"source": source}).to_string())
        .expect("exec arguments should decode")
        .expect("exec should be locally owned");
    let generic = plan
        .decode_local_function_call(
            "extension_freeform",
            &serde_json::json!({"input": generic_input}).to_string(),
        )
        .expect("generic arguments should decode")
        .expect("generic freeform should be locally owned");

    assert_eq!(exec.input, GrokLocalToolInput::Freeform(source.to_string()));
    assert_eq!(
        generic.input,
        GrokLocalToolInput::Freeform(generic_input.to_string())
    );
    assert!(
        plan.declarations
            .iter()
            .all(|tool| matches!(tool, ToolSpec::Function(_)))
    );
    for (tool_name, original_description) in [
        ("exec", "Run exec locally."),
        ("extension_freeform", "Run extension_freeform locally."),
    ] {
        let description = plan
            .declarations
            .iter()
            .find_map(|tool| match tool {
                ToolSpec::Function(tool) if tool.name == tool_name => Some(&tool.description),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing projected declaration for {tool_name}"));
        assert!(description.starts_with(original_description));
        assert!(description.contains("descriptive only"));
        assert!(
            description
                .contains(r#"{"type":"grammar","syntax":"lark","definition":"start: /.+/s"}"#)
        );
    }
    let wire = serde_json::to_string(&plan.declarations).expect("serialize planned declarations");
    assert!(!wire.contains("\"type\":\"custom\""));
    assert!(!wire.contains("\"type\":\"tool_search\""));
    assert!(!wire.contains("\"type\":\"namespace\""));
}

#[test]
fn freeform_description_preserves_boundary_and_complete_grammar_metadata() {
    let description =
        "Original description.\n\nMetadata-like text: {\"type\":\"not authoritative\"}.";
    let format = FreeformToolFormat {
        r#type: "grammar\"type".to_string(),
        syntax: "lark\n未来".to_string(),
        definition: "start: \"quoted\"\n  | /雪+/".to_string(),
    };
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("extension_freeform"),
        spec: ToolSpec::Freeform(FreeformTool {
            name: "extension_freeform".to_string(),
            description: description.to_string(),
            defer_loading: None,
            format: format.clone(),
        }),
    }])
    .expect("complete freeform metadata should be representable");

    let ToolSpec::Function(projected) = &plan.declarations[0] else {
        panic!("freeform projection must be model-visible as one function");
    };
    let serialized_format = serde_json::to_string(&format).expect("test format should serialize");
    assert_eq!(
        projected.description,
        format!(
            "{description}\n\nLocal Codex freeform grammar metadata (descriptive only; the Grok Gateway does not enforce this grammar):\n{serialized_format}"
        )
    );
}

#[test]
fn freeform_grammar_above_former_boundary_is_preserved_without_truncation() {
    let description = "Must remain complete.";
    let format = FreeformToolFormat {
        r#type: "grammar".to_string(),
        syntax: "lark".to_string(),
        definition: "x".repeat(8_001),
    };
    let plan = plan_grok_tools(vec![GrokLocalTool {
        identity: ToolName::plain("oversized_freeform"),
        spec: ToolSpec::Freeform(FreeformTool {
            name: "oversized_freeform".to_string(),
            description: description.to_string(),
            defer_loading: None,
            format: format.clone(),
        }),
    }])
    .expect("the former standalone boundary must not reject a representable freeform tool");

    let ToolSpec::Function(projected) = &plan.declarations[0] else {
        panic!("freeform projection must be model-visible as one function");
    };
    let serialized_format = serde_json::to_string(&format).expect("test format should serialize");
    assert_eq!(
        projected.description,
        format!(
            "{description}\n\nLocal Codex freeform grammar metadata (descriptive only; the Grok Gateway does not enforce this grammar):\n{serialized_format}"
        )
    );
    assert!(projected.description.len() > 8_000);
}

#[test]
fn malformed_freeform_wrapper_and_unprojectable_tool_fail_closed() {
    let plan = plan_grok_tools(vec![freeform_tool("extension_freeform")])
        .expect("generic freeform should be plannable");
    assert!(matches!(
        plan.decode_local_function_call(
            "extension_freeform",
            r#"{"input":"ok","unexpected":true}"#,
        ),
        Err(GrokToolCallDecodeError::InvalidFunctionArguments { .. })
    ));

    let error = plan_grok_tools(vec![
        GrokLocalTool {
            identity: ToolName::plain("safe_function"),
            spec: ToolSpec::Function(ResponsesApiTool {
                name: "safe_function".to_string(),
                description: "This entry must not form a reduced plan.".to_string(),
                strict: true,
                defer_loading: None,
                parameters: JsonSchema::object(
                    BTreeMap::new(),
                    Some(Vec::new()),
                    Some(false.into()),
                ),
                output_schema: None,
            }),
        },
        GrokLocalTool {
            identity: ToolName::plain("unsupported"),
            spec: ToolSpec::ImageGeneration,
        },
    ])
    .expect_err("one unprojectable capability must fail the complete plan");
    assert!(matches!(
        error,
        GrokToolPlanError::UnsupportedLocalTool { identity, .. }
            if identity == ToolName::plain("unsupported")
    ));
}

fn namespaced_function(namespace: &str, name: &str) -> GrokLocalTool {
    GrokLocalTool {
        identity: ToolName::namespaced(namespace, name),
        spec: ToolSpec::Namespace(ResponsesApiNamespace {
            name: namespace.to_string(),
            description: format!("{namespace} tools."),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: name.to_string(),
                description: format!("Run {namespace}.{name}."),
                strict: true,
                defer_loading: Some(true),
                parameters: JsonSchema::object(
                    BTreeMap::new(),
                    Some(Vec::new()),
                    Some(false.into()),
                ),
                output_schema: None,
            })],
        }),
    }
}

fn freeform_tool(name: &str) -> GrokLocalTool {
    GrokLocalTool {
        identity: ToolName::plain(name),
        spec: ToolSpec::Freeform(FreeformTool {
            name: name.to_string(),
            description: format!("Run {name} locally."),
            defer_loading: Some(true),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: /.+/s".to_string(),
            },
        }),
    }
}

fn wire_name_for(plan: &GrokToolPlan, identity: &ToolName) -> String {
    plan.local_routes
        .iter()
        .find_map(|(wire_name, route)| {
            (route.canonical_identity == *identity).then(|| wire_name.clone())
        })
        .unwrap_or_else(|| panic!("missing wire route for {identity}"))
}
