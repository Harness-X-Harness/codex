use super::ModelInfo;
use super::StructuredEditToolType;
use super::tests::test_model;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn structured_edit_capability_is_optional_and_round_trips() {
    let stock = test_model(/*spec*/ None);
    let mut value = serde_json::to_value(&stock).expect("serialize stock metadata");
    assert!(value.get("structured_edit_tool_type").is_none());
    assert_eq!(
        serde_json::from_value::<ModelInfo>(value.clone()).unwrap(),
        stock
    );
    value["structured_edit_tool_type"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::from_value::<ModelInfo>(value.clone()).unwrap(),
        stock
    );

    value["structured_edit_tool_type"] = json!("exact_match");
    let enabled: ModelInfo = serde_json::from_value(value).unwrap();
    let expected = ModelInfo {
        structured_edit_tool_type: Some(StructuredEditToolType::ExactMatch),
        ..stock
    };
    assert_eq!(enabled, expected);
    assert_eq!(
        serde_json::from_str::<ModelInfo>(&serde_json::to_string(&enabled).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn structured_edit_capability_rejects_unknown_values() {
    let mut value = serde_json::to_value(test_model(/*spec*/ None)).unwrap();
    value["structured_edit_tool_type"] = json!("fuzzy_match");
    assert!(serde_json::from_value::<ModelInfo>(value).is_err());
}
