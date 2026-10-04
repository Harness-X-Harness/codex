use super::AdditionalProperties;
use super::JsonSchema;
use super::parse_tool_input_schema;
use super::parse_tool_input_schema_with_max_bytes;
use super::parse_tool_input_schema_without_compaction;
use crate::ResponsesApiNamespace;
use crate::ResponsesApiNamespaceTool;
use crate::ToolName;
use crate::ToolSpec;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

fn dynamic(input_schema: Value) -> DynamicToolFunctionSpec {
    DynamicToolFunctionSpec {
        name: "bounded".to_string(),
        description: "Bounded lookup".to_string(),
        input_schema,
        defer_loading: false,
    }
}

fn mcp(input_schema: Value) -> rmcp::model::Tool {
    rmcp::model::Tool::new(
        "bounded",
        "Bounded lookup",
        Arc::new(rmcp::model::object(input_schema)),
    )
}

fn argument_schema() -> Value {
    json!({
        "type":"object", "properties":{
            "count":{"type":"integer","minimum":1,"maximum":5},
            "images":{"type":"array","items":{"type":"string"},"maxItems":3}
        }
    })
}

#[test]
fn numeric_and_array_bounds_round_trip_through_nested_nullable_and_referenced_schemas() {
    let source = json!({
        "type":"object",
        "properties":{
            "integer":{"type":"integer","minimum":-9007199254740993_i64,"maximum":u64::MAX},
            "fraction":{"type":"number","minimum":-0.25,"maximum":1.5},
            "contradiction":{"type":"number","minimum":10,"maximum":1},
            "array_contradiction":{"type":"array","items":{"type":"string"},"minItems":5,"maxItems":3},
            "irrelevant":{"type":"string","minimum":10,"maximum":1,"maxItems":3},
            "nullable":{"anyOf":[
                {"type":["array","null"],"items":{"$ref":"#/$defs/Count"},"minItems":0,"maxItems":3},
                {"type":"null"}
            ]},
            "legacy":{"$ref":"#/definitions/Array"}
        },
        "additionalProperties":{"type":"array","items":{"type":"number","minimum":-1.5,"maximum":2.5},"maxItems":2},
        "$defs":{"Count":{"type":"integer","minimum":1,"maximum":5}},
        "definitions":{"Array":{"type":"array","items":{"type":"string"},"maxItems":4}}
    });
    for schema in [
        serde_json::from_value::<JsonSchema>(source.clone()).expect("direct Value"),
        serde_json::from_str(&source.to_string()).expect("direct JSON"),
        parse_tool_input_schema(&source).expect("ordinary normalization"),
        parse_tool_input_schema_without_compaction(&source).expect("trusted normalization"),
    ] {
        assert_eq!(
            serde_json::to_value(&schema).expect("serialize schema"),
            source
        );
        assert_eq!(
            serde_json::from_value::<JsonSchema>(source.clone()).expect("round trip"),
            schema
        );
    }
}

#[test]
fn max_items_accepts_exact_whole_numbers_from_json_and_value() {
    for (token, expected) in [
        ("0", 0),
        ("-0.0", 0),
        ("3", 3),
        ("3.0", 3),
        ("3e0", 3),
        ("4.2e1", 42),
        ("9007199254740993", 9_007_199_254_740_993),
        ("18446744073709551615", u64::MAX),
    ] {
        let json = format!(r#"{{"type":"array","items":{{"type":"string"}},"maxItems":{token}}}"#);
        let value: Value = serde_json::from_str(&json).expect("JSON input");
        let expected = json!({"type":"array","items":{"type":"string"},"maxItems":expected});
        for schema in [
            serde_json::from_str::<JsonSchema>(&json).expect("raw whole number"),
            serde_json::from_value(value.clone()).expect("Value whole number"),
            parse_tool_input_schema(&value).expect("normalized whole number"),
        ] {
            assert_eq!(
                serde_json::to_value(schema).expect("schema JSON"),
                expected,
                "{token}"
            );
        }
    }
}

#[test]
fn raw_max_items_keeps_decimal_precision_in_additional_properties() {
    // The direct JSON route retains lexical precision. The Value route retains
    // the number representation already provided by serde_json, not lost input digits.
    for (token, expected) in [
        ("9007199254740993.0", 9_007_199_254_740_993),
        ("9.007199254740993e15", 9_007_199_254_740_993),
        ("18446744073709551615.0", u64::MAX),
        ("184467440737095516150e-1", u64::MAX),
    ] {
        let json = format!(
            r#"{{"type":"object","properties":{{}},"additionalProperties":{{"type":"array","items":{{"type":"string"}},"maxItems":{token}}}}"#
        );
        let schema: JsonSchema = serde_json::from_str(&json).expect("nested exact whole number");
        let expected = json!({"type":"object","properties":{},"additionalProperties":{
            "type":"array","items":{"type":"string"},"maxItems":expected
        }});
        assert_eq!(
            serde_json::to_value(&schema).expect("nested schema"),
            expected
        );
        assert_eq!(
            serde_json::from_value::<JsonSchema>(expected.clone())
                .expect("nested Value round trip"),
            schema
        );
        assert_eq!(
            serde_json::to_value(
                parse_tool_input_schema(&expected).expect("nested normalized bound")
            )
            .expect("schema"),
            expected
        );
    }
    for token in [
        "1.0000000000000001",
        "9007199254740993.1",
        "18446744073709551616",
        "1e10000",
        "1e-10000",
    ] {
        let json = format!(r#"{{"additionalProperties":{{"maxItems":{token}}}}}"#);
        assert!(
            serde_json::from_str::<JsonSchema>(&json).is_err(),
            "{token}"
        );
    }
}

#[test]
fn malformed_recognized_bounds_fail_direct_trusted_compacted_and_tool_conversion_routes() {
    for field in ["minimum", "maximum", "maxItems"] {
        let mut invalid = vec![json!("3"), json!(true), json!([]), json!({})];
        if field == "maxItems" {
            invalid.extend([
                json!(-1),
                json!(1.5),
                serde_json::from_str("18446744073709551616").expect("JSON number beyond u64"),
            ]);
        }
        for value in invalid {
            let mut bound = json!({"type":"array","items":{"type":"string"}});
            bound[field] = value;
            let source = json!({"type":"object","properties":{"bounded":bound.clone()},"additionalProperties":bound});
            assert!(serde_json::from_str::<JsonSchema>(&source.to_string()).is_err());
            assert!(serde_json::from_value::<JsonSchema>(source.clone()).is_err());
            assert!(parse_tool_input_schema(&source).is_err());
            assert!(parse_tool_input_schema_without_compaction(&source).is_err());
            assert!(parse_tool_input_schema_with_max_bytes(&source, /*max_bytes*/ 0).is_err());
            let dynamic = dynamic(source.clone());
            let mcp = mcp(source);
            assert!(crate::parse_dynamic_tool(&dynamic).is_err());
            assert!(crate::dynamic_tool_to_responses_api_tool(&dynamic).is_err());
            assert!(crate::parse_mcp_tool(&mcp).is_err());
            assert!(crate::parse_agent_plugin_mcp_tool(&mcp).is_err());
            assert!(
                crate::mcp_tool_to_deferred_responses_api_tool(&ToolName::plain("bounded"), &mcp)
                    .is_err()
            );
            assert!(
                crate::mcp_tool_to_responses_api_tool(
                    &ToolName::plain("bounded"),
                    &mcp,
                    /*schema_max_bytes*/ None
                )
                .is_err()
            );
            assert!(
                crate::agent_plugin_mcp_tool_to_responses_api_tool(
                    &ToolName::plain("bounded"),
                    &mcp
                )
                .is_err()
            );
        }
    }
}

#[test]
fn absent_null_and_unsupported_constraints_keep_existing_sanitizer_behavior() {
    for source in [
        json!({"type":"number"}),
        json!({"type":"number","minimum":null,"maximum":null,"maxItems":null}),
        json!({"type":"number","exclusiveMinimum":1,"exclusiveMaximum":4,"multipleOf":2}),
    ] {
        assert_eq!(
            serde_json::to_value(parse_tool_input_schema(&source).expect("schema"))
                .expect("schema JSON"),
            json!({"type":"number"})
        );
    }
    for source in [
        json!({"maxItems":3}),
        json!({"maxItems":"invalid"}),
        json!({"minItems":1}),
        json!({"description":"untyped"}),
    ] {
        assert_eq!(
            parse_tool_input_schema(&source).expect("unchanged untyped schema"),
            JsonSchema::default()
        );
    }
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema(&json!({"maximum":3})).expect("numeric inference")
        )
        .expect("schema"),
        json!({"type":"number","maximum":3})
    );
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema(&json!({"multipleOf":3})).expect("unchanged inference")
        )
        .expect("schema"),
        json!({"type":"number"})
    );
    for additional in [json!(false), json!(true), json!({"type":"string"})] {
        let source = json!({"type":"object","properties":{},"additionalProperties":additional});
        assert_eq!(
            serde_json::to_value(parse_tool_input_schema(&source).expect("additional properties"))
                .expect("schema"),
            source
        );
    }
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema(&json!({"type":"object","additionalProperties":null}))
                .expect("null optional additional properties")
        )
        .expect("schema"),
        json!({"type":"object","properties":{}})
    );
    for additional in [json!(1), json!("false")] {
        assert!(
            parse_tool_input_schema(&json!({"type":"object","additionalProperties":additional}))
                .is_err()
        );
    }
}

#[test]
fn additional_properties_preserves_legacy_struct_sequences_and_missing_field_errors() {
    let empty = vec![Value::Null; 15];
    assert_eq!(
        serde_json::from_value::<AdditionalProperties>(json!(empty))
            .expect("legacy empty schema sequence"),
        AdditionalProperties::Schema(Box::default())
    );
    let mut legacy = vec![Value::Null; 15];
    legacy[1] = json!("number");
    legacy[2] = json!("legacy description");
    legacy[8] = json!(["late-field"]);
    let source = json!({"type":"object","properties":{},"additionalProperties":legacy});
    let expected = json!({"type":"object","properties":{},"additionalProperties":{
        "type":"number","description":"legacy description","required":["late-field"]
    }});
    for schema in [
        serde_json::from_str::<JsonSchema>(&source.to_string()).expect("legacy JSON sequence"),
        serde_json::from_value(source.clone()).expect("legacy Value sequence"),
        parse_tool_input_schema(&source).expect("legacy normalized sequence"),
    ] {
        assert_eq!(
            serde_json::to_value(schema).expect("legacy schema"),
            expected
        );
    }
    legacy.extend([json!(3), json!(-1.5), json!(4.5)]);
    let extended = json!({"additionalProperties":legacy});
    let schema: JsonSchema = serde_json::from_value(extended).expect("appended sequence bounds");
    assert_eq!(
        serde_json::to_value(schema).expect("extended schema"),
        json!({"additionalProperties":{
            "type":"number","description":"legacy description","required":["late-field"],"maxItems":3,"minimum":-1.5,"maximum":4.5
        }})
    );
    for invalid in [vec![], vec![Value::Null; 14], vec![Value::Null; 19]] {
        let source = json!({"type":"object","properties":{},"additionalProperties":invalid});
        assert!(serde_json::from_str::<JsonSchema>(&source.to_string()).is_err());
        assert!(serde_json::from_value::<JsonSchema>(source.clone()).is_err());
        assert!(parse_tool_input_schema(&source).is_err());
    }
    for invalid in [Value::Null, json!("false"), json!(1)] {
        assert!(serde_json::from_value::<AdditionalProperties>(invalid.clone()).is_err());
        assert!(serde_json::from_str::<AdditionalProperties>(&invalid.to_string()).is_err());
    }
    for valid in [json!(false), json!(true), json!({})] {
        let parsed: AdditionalProperties =
            serde_json::from_value(valid.clone()).expect("legacy additional properties");
        assert_eq!(
            serde_json::to_value(parsed).expect("additional properties JSON"),
            valid
        );
    }
}

#[test]
fn additional_properties_keeps_the_supported_schema_depth_boundary() {
    let mut shallow = json!({"type":"array","items":{"type":"string"},"maxItems":3});
    for _ in 0..16 {
        shallow = json!({"type":"object","properties":{},"additionalProperties":shallow});
    }
    let decoded: JsonSchema =
        serde_json::from_str(&shallow.to_string()).expect("bounded supported nesting");
    assert_eq!(
        serde_json::to_value(decoded).expect("nested schema"),
        shallow
    );
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema_without_compaction(&shallow).expect("trusted nested schema")
        )
        .expect("schema"),
        shallow
    );
    let deep = format!(
        "{}{{}}{}",
        r#"{"additionalProperties":"#.repeat(160),
        "}".repeat(160)
    );
    assert!(serde_json::from_str::<JsonSchema>(&deep).is_err());
    assert!(
        serde_json::from_str::<Value>(&deep).is_err(),
        "production Value admission retains its upstream depth boundary"
    );
}

#[test]
fn direct_raw_unknown_fields_use_owner_skipping_but_value_admission_keeps_depth_limits() {
    let expected =
        json!({"type":"object","properties":{},"additionalProperties":{"type":"number"}});
    for depth in [8, 160] {
        let ignored = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let raw = format!(
            r#"{{"type":"object","properties":{{}},"additionalProperties":{{"type":"number","future":{ignored}}}}}"#
        );
        let schema: JsonSchema =
            serde_json::from_str(&raw).expect("owner skips unknown raw subtree");
        assert_eq!(
            serde_json::to_value(schema).expect("known schema"),
            expected
        );
        if depth == 8 {
            let value: Value = serde_json::from_str(&raw).expect("bounded Value input");
            for schema in [
                parse_tool_input_schema(&value).expect("ordinary Value route"),
                parse_tool_input_schema_without_compaction(&value).expect("trusted Value route"),
            ] {
                assert_eq!(
                    serde_json::to_value(schema).expect("normalized schema"),
                    expected
                );
            }
        } else {
            let error = serde_json::from_str::<Value>(&raw)
                .expect_err("production Value boundary rejects deep input");
            assert!(
                error.to_string().contains("recursion limit exceeded"),
                "{error}"
            );
        }
    }
}

#[test]
fn compaction_counts_preserved_bounds_at_the_exact_utf8_budget() {
    let mut source = argument_schema();
    source["description"] = json!("guidance é");
    let bytes = serde_json::to_vec(&source).expect("source JSON").len();
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema_with_max_bytes(&source, bytes).expect("exact budget")
        )
        .expect("schema"),
        source
    );
    let compacted = parse_tool_input_schema_with_max_bytes(&source, bytes - 1).expect("compaction");
    let mut expected = source.clone();
    expected
        .as_object_mut()
        .expect("object")
        .remove("description");
    assert_eq!(
        serde_json::to_value(compacted).expect("compacted schema"),
        expected
    );
    for (limit, expected) in [(bytes, &source), (bytes - 1, &expected)] {
        let tool = crate::mcp_tool_to_responses_api_tool(
            &ToolName::plain("bounded"),
            &mcp(source.clone()),
            Some(limit),
        )
        .expect("MCP schema budget");
        assert_eq!(
            serde_json::to_value(tool.parameters).expect("MCP schema"),
            *expected
        );
    }
    source["properties"]["count"]
        .as_object_mut()
        .expect("count")
        .remove("minimum");
    source["properties"]["count"]
        .as_object_mut()
        .expect("count")
        .remove("maximum");
    source["properties"]["images"]
        .as_object_mut()
        .expect("images")
        .remove("maxItems");
    assert_eq!(
        serde_json::to_value(
            parse_tool_input_schema_with_max_bytes(&source, bytes - 1)
                .expect("bounds-free control")
        )
        .expect("schema"),
        source
    );
}

#[test]
fn mcp_and_dynamic_bounds_reach_responses_and_code_mode_without_changing_argument_types() {
    let source = argument_schema();
    let mcp = mcp(source.clone());
    let dynamic = dynamic(source.clone());
    let definitions = [
        crate::parse_mcp_tool(&mcp).expect("MCP"),
        crate::parse_dynamic_tool(&dynamic).expect("dynamic"),
    ];
    for definition in definitions {
        assert_eq!(
            serde_json::to_value(&definition.input_schema).expect("definition schema"),
            source
        );
        let tool = crate::tool_definition_to_responses_api_tool(definition);
        let direct =
            crate::create_tools_json_for_responses_api(&[ToolSpec::Function(tool.clone())])
                .expect("Responses JSON");
        assert_eq!(direct[0]["parameters"], source);
        let namespace = ToolSpec::Namespace(ResponsesApiNamespace {
            name: "images".to_string(),
            description: "Image operations".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(tool.clone())],
        });
        let code = crate::collect_code_mode_tool_definitions(
            [&namespace],
            /*code_mode_input_schema_max_bytes*/ None,
        );
        assert_eq!(code.len(), 1);
        assert_eq!(code[0].input_schema, Some(source.clone()));
        assert!(code[0].description.contains("count?: number;"));
        let mut unbounded = source.clone();
        unbounded["properties"]["count"] = json!({"type":"integer"});
        unbounded["properties"]["images"] = json!({"type":"array","items":{"type":"string"}});
        let mut unbounded_tool = tool;
        unbounded_tool.parameters = parse_tool_input_schema(&unbounded).expect("unbounded control");
        let unbounded = ToolSpec::Namespace(ResponsesApiNamespace {
            name: "images".to_string(),
            description: "Image operations".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(unbounded_tool)],
        });
        let control = crate::collect_code_mode_tool_definitions(
            [&unbounded],
            /*code_mode_input_schema_max_bytes*/ None,
        );
        assert_eq!(code[0].description, control[0].description);
    }
}

#[test]
fn agent_plugin_full_tool_budget_includes_bounds() {
    let mut source = json!({"type":"object","properties":{}});
    for index in 0..80 {
        source["properties"][format!("value{index}")] =
            json!({"type":"integer","minimum":0,"maximum":u64::MAX});
    }
    source["properties"][""] = json!({"type":"string"});
    let wrapper =
        json!({"name":"bounded","description":"Bounded lookup","strict":false,"parameters":source});
    let size = serde_json::to_vec(&wrapper).expect("full tool JSON").len();
    assert!(size < 8000);
    for extra in [0, 1] {
        let mut source = source.clone();
        let properties = source["properties"].as_object_mut().expect("properties");
        let value = properties.remove("").expect("padding property");
        properties.insert("p".repeat(8000 - size + extra), value);
        let expected_size = serde_json::to_vec(&json!({"name":"bounded","description":"Bounded lookup","strict":false,"parameters":source})).expect("full tool").len();
        assert_eq!(expected_size, 8000 + extra);
        let tool = crate::agent_plugin_mcp_tool_to_responses_api_tool(
            &ToolName::plain("bounded"),
            &mcp(source.clone()),
        )
        .expect("plugin tool");
        let expected = if extra == 0 {
            source.clone()
        } else {
            json!({"type":"object","properties":{},"additionalProperties":true})
        };
        assert_eq!(
            serde_json::to_value(tool.parameters).expect("plugin schema"),
            expected
        );
        for property in source["properties"]
            .as_object_mut()
            .expect("properties")
            .values_mut()
        {
            property
                .as_object_mut()
                .expect("property")
                .remove("minimum");
            property
                .as_object_mut()
                .expect("property")
                .remove("maximum");
        }
        let control = crate::agent_plugin_mcp_tool_to_responses_api_tool(
            &ToolName::plain("bounded"),
            &mcp(source.clone()),
        )
        .expect("bounds-free plugin");
        assert_eq!(
            serde_json::to_value(control.parameters).expect("control schema"),
            source
        );
    }
}

#[test]
fn code_mode_retains_large_bounded_schemas_without_inflating_argument_types() {
    let properties: serde_json::Map<String, Value> = (0..256)
        .map(|index| {
            (
                format!("value{index}"),
                json!({"type":"integer","minimum":0,"maximum":u64::MAX}),
            )
        })
        .collect();
    let source = json!({"type":"object","properties":properties});
    let source_bytes = serde_json::to_vec(&source).expect("schema").len();
    assert!(source_bytes > codex_code_mode::DEFAULT_INPUT_SCHEMA_MAX_BYTES);
    let spec = ToolSpec::Function(
        crate::dynamic_tool_to_responses_api_tool(&dynamic(source.clone())).expect("dynamic spec"),
    );
    for limit in [None, Some(source_bytes)] {
        let definitions = crate::collect_code_mode_tool_definitions([&spec], limit);
        assert_eq!(definitions[0].input_schema, Some(source.clone()));
        assert_eq!(
            definitions[0].input_schema_max_bytes,
            Some(limit.unwrap_or(codex_code_mode::DEFAULT_INPUT_SCHEMA_MAX_BYTES))
        );
        assert!(
            definitions[0].description.contains("value255?: number;"),
            "{}",
            definitions[0].description
        );
    }
}
