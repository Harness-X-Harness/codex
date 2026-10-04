use super::StructuredEditArgs;
use crate::tools::handlers::parse_arguments;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn structured_edit_arguments_preserve_exact_strings_and_default_replace_all() {
    let arguments = json!({"file_path":"a.txt", "old_string":"é\r\n", "new_string":"e\u{301}\n"});
    assert_eq!(
        parse_arguments::<StructuredEditArgs>(&arguments.to_string()).unwrap(),
        StructuredEditArgs {
            file_path: "a.txt".to_string(),
            old_string: "é\r\n".to_string(),
            new_string: "e\u{301}\n".to_string(),
            replace_all: false,
            environment_id: None,
        }
    );
    let mut explicit = arguments;
    explicit["replace_all"] = json!(true);
    explicit["environment_id"] = json!("remote");
    let parsed = parse_arguments::<StructuredEditArgs>(&explicit.to_string()).unwrap();
    assert!(parsed.replace_all);
    assert_eq!(parsed.environment_id.as_deref(), Some("remote"));
}

#[test]
fn structured_edit_arguments_reject_unknown_fields_and_wrong_types() {
    let arguments = json!({"file_path":"a.txt", "old_string":"old", "new_string":"new"});
    for (field, value) in [
        ("replace_all", json!("true")),
        ("replace_all", json!(null)),
        ("file_path", json!(7)),
        ("old_string", json!(null)),
        ("new_string", json!([])),
        ("environment_id", json!(7)),
        ("command", json!("ignored input")),
    ] {
        let mut invalid = arguments.clone();
        invalid[field] = value;
        assert!(
            parse_arguments::<StructuredEditArgs>(&invalid.to_string()).is_err(),
            "{field}"
        );
    }
}
