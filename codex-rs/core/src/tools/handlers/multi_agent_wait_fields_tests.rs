use super::WaitArgs;
use pretty_assertions::assert_eq;

#[test]
fn timeout_whole_numbers_keep_exact_signed_values() {
    for (token, expected) in [
        ("1", 1),
        ("10000.0", 10_000),
        ("1e4", 10_000),
        ("-1.0", -1),
        ("-0e10000", 0),
        ("9007199254740993.0", 9_007_199_254_740_993),
        ("92233720368547758070e-1", i64::MAX),
        ("-9223372036854775808.0", i64::MIN),
    ] {
        let args: WaitArgs = serde_json::from_str(&format!(r#"{{"timeout_ms":{token}}}"#))
            .expect("exact whole timeout should parse");
        assert_eq!(args.timeout_ms, Some(expected), "{token}");
    }
}

#[test]
fn timeout_missing_and_null_remain_unspecified() {
    for input in ["{}", r#"{"timeout_ms":null}"#] {
        let args: WaitArgs = serde_json::from_str(input).expect("optional timeout should parse");
        assert_eq!(args.timeout_ms, None);
    }
}

#[test]
fn timeout_rejects_fraction_range_and_nonnumber_inputs() {
    for token in [
        "1.0000000000000001",
        "9223372036854775808",
        "-9223372036854775809.0",
        "1e10000",
        "1e-10000",
        "true",
        r#""1""#,
        "[]",
        "{}",
        r#"{"$serde_json::private::Number":"1"}"#,
        r#"{"$serde_json::private::RawValue":"1"}"#,
    ] {
        assert!(
            serde_json::from_str::<WaitArgs>(&format!(r#"{{"timeout_ms":{token}}}"#)).is_err(),
            "{token}"
        );
    }
}
