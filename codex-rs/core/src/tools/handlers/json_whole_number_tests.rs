use super::deserialize_option_whole_i64;
use super::deserialize_optional_string_or_whole_count;
use super::deserialize_whole_i32;
use super::deserialize_whole_u64;
use pretty_assertions::assert_eq;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct OptionalI64 {
    #[serde(default, deserialize_with = "deserialize_option_whole_i64")]
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct RequiredU64 {
    #[serde(deserialize_with = "deserialize_whole_u64")]
    yield_time_ms: u64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct RequiredI32 {
    #[serde(deserialize_with = "deserialize_whole_i32")]
    session_id: i32,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct OptionalForkTurns {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_or_whole_count"
    )]
    fork_turns: Option<String>,
}

fn parse_i64(json: &str) -> Result<OptionalI64, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

fn parse_u64(json: &str) -> Result<RequiredU64, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

fn parse_i32(json: &str) -> Result<RequiredI32, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

fn parse_fork_turns(json: &str) -> Result<OptionalForkTurns, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

#[test]
fn accepts_integer_and_whole_decimal_tokens() {
    assert_eq!(
        parse_i64(r#"{"timeout_ms":180000}"#).expect("integer"),
        OptionalI64 {
            timeout_ms: Some(180000)
        }
    );
    assert_eq!(
        parse_i64(r#"{"timeout_ms":180000.0}"#).expect("whole decimal"),
        OptionalI64 {
            timeout_ms: Some(180000)
        }
    );
    assert_eq!(
        parse_i64(r#"{}"#).expect("omitted"),
        OptionalI64 { timeout_ms: None }
    );
}

#[test]
fn accepts_exact_integers_beyond_binary64_range() {
    assert_eq!(
        parse_i64(r#"{"timeout_ms":9007199254740993.0}"#).expect("2^53+1 must stay exact"),
        OptionalI64 {
            timeout_ms: Some(9007199254740993)
        }
    );
    assert_eq!(
        parse_u64(r#"{"yield_time_ms":9007199254740993.0}"#)
            .expect("2^53+1 must stay exact for u64"),
        RequiredU64 {
            yield_time_ms: 9007199254740993
        }
    );
}

#[test]
fn accepts_decimal_and_exponent_whole_forms() {
    assert_eq!(
        parse_i64(r#"{"timeout_ms":1e3}"#).expect("1e3"),
        OptionalI64 {
            timeout_ms: Some(1000)
        }
    );
    assert_eq!(
        parse_i64(r#"{"timeout_ms":1.20e2}"#).expect("1.20e2"),
        OptionalI64 {
            timeout_ms: Some(120)
        }
    );
    assert_eq!(
        parse_i64(r#"{"timeout_ms":-0.0}"#).expect("-0.0"),
        OptionalI64 {
            timeout_ms: Some(0)
        }
    );
}

#[test]
fn rejects_fractional_forms() {
    for json in [
        r#"{"timeout_ms":1.1}"#,
        r#"{"timeout_ms":1e-1}"#,
        r#"{"timeout_ms":1.0000000000000001}"#,
    ] {
        let err = parse_i64(json).expect_err(json);
        assert!(
            err.contains("must be a finite whole number"),
            "{json} -> {err}"
        );
        assert!(!err.contains("expected i64"), "{json} -> {err}");
        assert!(!err.contains("invalid type: map"), "{json} -> {err}");
    }
}

#[test]
fn signed_and_unsigned_boundaries_are_exact() {
    assert_eq!(
        parse_i64(&format!(r#"{{"timeout_ms":{}.0}}"#, i64::MAX)).expect("i64::MAX"),
        OptionalI64 {
            timeout_ms: Some(i64::MAX)
        }
    );
    assert_eq!(
        parse_i64(&format!(r#"{{"timeout_ms":{}.0}}"#, i64::MIN)).expect("i64::MIN"),
        OptionalI64 {
            timeout_ms: Some(i64::MIN)
        }
    );
    assert_eq!(
        parse_u64(&format!(r#"{{"yield_time_ms":{}.0}}"#, u64::MAX)).expect("u64::MAX"),
        RequiredU64 {
            yield_time_ms: u64::MAX
        }
    );
    assert_eq!(
        parse_i32(&format!(r#"{{"session_id":{}.0}}"#, i32::MAX)).expect("i32::MAX"),
        RequiredI32 {
            session_id: i32::MAX
        }
    );

    let over_i64 = parse_i64(r#"{"timeout_ms":9223372036854775808.0}"#).expect_err("i64::MAX + 1");
    assert!(
        over_i64.contains("must be a finite whole number"),
        "{over_i64}"
    );
    let over_u64 =
        parse_u64(r#"{"yield_time_ms":18446744073709551616.0}"#).expect_err("u64::MAX + 1");
    assert!(
        over_u64.contains("must be a finite whole number"),
        "{over_u64}"
    );
    let negative_u64 = parse_u64(r#"{"yield_time_ms":-1}"#).expect_err("negative u64");
    assert!(
        negative_u64.contains("must be a finite whole number"),
        "{negative_u64}"
    );
    let over_i32 = parse_i32(r#"{"session_id":2147483648.0}"#).expect_err("i32::MAX + 1");
    assert!(
        over_i32.contains("must be a finite whole number"),
        "{over_i32}"
    );
}

#[test]
fn fork_turns_inherits_exact_whole_number_conversion() {
    assert_eq!(
        parse_fork_turns(r#"{"fork_turns":3}"#).expect("integer"),
        OptionalForkTurns {
            fork_turns: Some("3".to_string())
        }
    );
    assert_eq!(
        parse_fork_turns(r#"{"fork_turns":3.0}"#).expect("whole decimal"),
        OptionalForkTurns {
            fork_turns: Some("3".to_string())
        }
    );
    assert_eq!(
        parse_fork_turns(r#"{"fork_turns":"none"}"#).expect("string none"),
        OptionalForkTurns {
            fork_turns: Some("none".to_string())
        }
    );
    let err = parse_fork_turns(r#"{"fork_turns":3.5}"#).expect_err("fractional count");
    assert!(err.contains("must be a finite whole number"), "{err}");
}
