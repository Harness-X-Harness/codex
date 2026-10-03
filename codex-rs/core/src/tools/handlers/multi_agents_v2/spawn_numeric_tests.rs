use super::*;
use pretty_assertions::assert_eq;

fn parse(fields: &str) -> Result<SpawnAgentArgs, serde_json::Error> {
    serde_json::from_str(&format!(
        r#"{{"message":"inspect this repo","task_name":"worker"{fields}}}"#,
    ))
}

#[test]
fn fork_turns_preserves_defaults_and_string_modes() {
    for (fields, expected) in [
        ("", None),
        (r#", "fork_turns": null"#, None),
        (r#", "fork_turns": """#, Some("")),
        (r#", "fork_turns": " \t""#, Some(" \t")),
        (r#", "fork_turns": "  AlL\t""#, Some("  AlL\t")),
    ] {
        let args = parse(fields).expect("default and string modes should parse");
        assert_eq!(args.fork_turns.as_deref(), expected);
        assert!(matches!(
            args.fork_mode(),
            Ok(Some(SpawnAgentForkMode::FullHistory))
        ));
    }
    let args = parse(r#", "fork_turns": "  NoNe\t""#).expect("none should parse");
    assert_eq!(args.fork_turns.as_deref(), Some("  NoNe\t"));
    assert!(matches!(args.fork_mode(), Ok(None)));
    let args = parse(r#", "fork_turns": "  1\t""#).expect("count string should parse");
    assert_eq!(args.fork_turns.as_deref(), Some("  1\t"));
    assert!(matches!(
        args.fork_mode(),
        Ok(Some(SpawnAgentForkMode::LastNTurns(1)))
    ));
}

#[test]
fn fork_turns_numeric_admission_preserves_positive_string_diagnostics() {
    for token in [r#""1.0""#, r#""1e0""#, r#""banana""#, r#""0""#, "0", "-0.0"] {
        let args = parse(&format!(r#", "fork_turns": {token}"#))
            .expect("strings and numeric zero should reach fork policy");
        assert_eq!(
            args.fork_mode().expect_err("invalid count must reject"),
            FunctionCallError::RespondToModel(
                "fork_turns must be `none`, `all`, or a positive integer string".to_string()
            ),
            "{token}"
        );
    }
    for token in ["true", "false"] {
        let args = parse(&format!(r#", "fork_turns": 1.0, "fork_context": {token}"#))
            .expect("legacy boolean should reach fork policy");
        assert_eq!(
            args.fork_mode().expect_err("fork_context must reject"),
            FunctionCallError::RespondToModel(
                "fork_context is not supported in MultiAgentV2; use fork_turns instead".to_string()
            )
        );
    }
    assert!(parse(r#", "fork_turns": 1.0, "unknown": true"#).is_err());
}

#[test]
fn fork_turns_numeric_usize_bounds_are_exact() {
    for count in [1, usize::MAX] {
        for token in [
            count.to_string(),
            format!("{count}.0"),
            format!("{count}e0"),
        ] {
            let args = parse(&format!(r#", "fork_turns": {token}"#))
                .expect("whole numeric count should parse");
            assert_eq!(args.fork_turns, Some(count.to_string()));
            assert!(matches!(
                args.fork_mode(),
                Ok(Some(SpawnAgentForkMode::LastNTurns(actual))) if actual == count
            ));
        }
    }
    let overflow = usize::MAX as u128 + 1;
    for token in [
        overflow.to_string(),
        format!("{overflow}.0"),
        format!("{overflow}e0"),
    ] {
        assert!(
            parse(&format!(r#", "fork_turns": {token}"#)).is_err(),
            "{token}"
        );
    }
}

#[test]
fn fork_turns_numeric_rejects_fractions_other_kinds_and_raw_sentinels() {
    for token in [
        "-1",
        "1.0000000000000001",
        "9007199254740993.1",
        "1e10000",
        "1e-10000",
        "true",
        "[]",
        "{}",
        r#"{"$serde_json::private::Number":"1"}"#,
        r#"{"$serde_json::private::RawValue":"1"}"#,
    ] {
        assert!(
            parse(&format!(r#", "fork_turns": {token}"#)).is_err(),
            "{token}"
        );
    }
}
