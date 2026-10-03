use super::WriteStdinArgs;
use super::parse_arguments;
use pretty_assertions::assert_eq;

#[test]
fn stdin_numeric_fields_preserve_exact_bounds_and_defaults() {
    for (session_id, expected) in [
        ("21474836470e-1", i32::MAX),
        ("-2.147483648e9", i32::MIN),
        ("-0.0", 0),
    ] {
        let max_output_tokens = usize::MAX;
        let args: WriteStdinArgs = parse_arguments(&format!(
            r#"{{"session_id":{session_id},"chars":"hello\n","yield_time_ms":184467440737095516150e-1,"max_output_tokens":{max_output_tokens}.0,"ignored":true}}"#
        ))
        .unwrap();
        assert_eq!(
            (
                args.session_id,
                args.chars,
                args.yield_time_ms,
                args.max_output_tokens
            ),
            (expected, "hello\n".to_owned(), u64::MAX, Some(usize::MAX)),
        );
    }
    for json in [
        r#"{"session_id":1}"#,
        r#"{"session_id":1.0,"max_output_tokens":null}"#,
    ] {
        let args: WriteStdinArgs = parse_arguments(json).unwrap();
        assert_eq!(
            (
                args.session_id,
                args.chars,
                args.yield_time_ms,
                args.max_output_tokens
            ),
            (1, String::new(), 250, None),
        );
    }
}

#[test]
fn stdin_numeric_fields_reject_fractions_wrong_kinds_and_out_of_range_values() {
    for field in ["session_id", "yield_time_ms", "max_output_tokens"] {
        let base = if field == "session_id" {
            ""
        } else {
            r#""session_id":1,"#
        };
        for token in [
            "1.0000000000000001",
            "9007199254740993.1",
            "1e-10000",
            "1e10000",
            "\"1\"",
            "true",
            "[]",
            "{}",
            r#"{"$serde_json::private::Number":"1"}"#,
            r#"{"$serde_json::private::RawValue":"1"}"#,
        ] {
            assert!(
                parse_arguments::<WriteStdinArgs>(&format!(r#"{{{base}"{field}":{token}}}"#))
                    .is_err(),
                "{field}={token}",
            );
        }
    }
    let usize_overflow = (usize::MAX as u128 + 1).to_string();
    for (field, token) in [
        ("session_id", "2147483648.0"),
        ("session_id", "-2147483649e0"),
        ("yield_time_ms", "18446744073709551616.0"),
        ("yield_time_ms", "-1"),
        ("max_output_tokens", usize_overflow.as_str()),
        ("max_output_tokens", "-1"),
    ] {
        let base = if field == "session_id" {
            ""
        } else {
            r#""session_id":1,"#
        };
        assert!(
            parse_arguments::<WriteStdinArgs>(&format!(r#"{{{base}"{field}":{token}}}"#)).is_err(),
            "{field}={token}",
        );
    }
    for json in [
        "{}",
        r#"{"session_id":null}"#,
        r#"{"session_id":1,"yield_time_ms":null}"#,
        r#"{"session_id":01}"#,
        r#"{"session_id":1,"yield_time_ms":0e+}"#,
        r#"{"session_id":1,"chars":null}"#,
        r#"{"session_id":1,"chars":7}"#,
    ] {
        assert!(parse_arguments::<WriteStdinArgs>(json).is_err(), "{json}");
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn stdin_numeric_limits_keep_bits_above_binary64() {
    let args: WriteStdinArgs = parse_arguments(
        r#"{"session_id":1e0,"yield_time_ms":9007199254740993.0,"max_output_tokens":9.007199254740993e15}"#,
    )
    .unwrap();
    assert_eq!(
        (args.yield_time_ms, args.max_output_tokens),
        (9_007_199_254_740_993, Some(9_007_199_254_740_993)),
    );
}
