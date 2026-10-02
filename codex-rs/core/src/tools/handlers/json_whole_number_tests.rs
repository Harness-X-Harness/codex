use super::from_raw;
use pretty_assertions::assert_eq;
use serde_json::value::RawValue;

fn parse<T: TryFrom<i128>>(token: &str) -> Result<T, String> {
    let raw: Box<RawValue> = serde_json::from_str(token).map_err(|err| err.to_string())?;
    from_raw(&raw).map_err(str::to_owned)
}

#[test]
fn equivalent_integer_forms_are_exact() {
    for (token, expected) in [
        ("180000", 180_000),
        ("180000.0", 180_000),
        ("1.8e5", 180_000),
        ("1.20E+2", 120),
        ("1000e-3", 1),
        ("1200e-2", 12),
        ("9.007199254740993e15", 9_007_199_254_740_993),
    ] {
        assert_eq!(parse::<u64>(token), Ok(expected), "{token}");
    }
    for expected in [
        9_007_199_254_740_991_u64,
        9_007_199_254_740_992,
        9_007_199_254_740_993,
    ] {
        for token in [
            expected.to_string(),
            format!("{expected}.0"),
            format!("{expected}e0"),
        ] {
            assert_eq!(parse::<u64>(&token), Ok(expected), "{token}");
        }
    }
}

#[test]
fn target_bounds_are_exact() {
    for token in ["2147483647", "2147483647.0", "2.147483647e9"] {
        assert_eq!(parse::<i32>(token), Ok(i32::MAX));
    }
    for token in ["-2147483648", "-2147483648.0", "-2.147483648e9"] {
        assert_eq!(parse::<i32>(token), Ok(i32::MIN));
    }
    for token in ["2147483648", "-2147483649.0"] {
        assert!(parse::<i32>(token).is_err(), "{token}");
    }
    for token in ["18446744073709551615", "184467440737095516150e-1"] {
        assert_eq!(parse::<u64>(token), Ok(u64::MAX));
    }
    for token in ["18446744073709551616", "18446744073709551616.0", "-1"] {
        assert!(parse::<u64>(token).is_err(), "{token}");
    }
    assert_eq!(parse::<usize>(&format!("{}.0", usize::MAX)), Ok(usize::MAX));
    assert!(parse::<usize>(&(usize::MAX as u128 + 1).to_string()).is_err());
}

#[test]
fn fractions_and_huge_nonzero_exponents_reject() {
    for token in [
        "1.0000000000000001",
        "9007199254740993.1",
        "1201e-2",
        "1e-10000",
        "1e10000",
        "123456789012345678901",
    ] {
        assert!(parse::<u64>(token).is_err(), "{token}");
    }
    let exponent = "9".repeat(10_000);
    for token in [format!("1e{exponent}"), format!("1e-{exponent}")] {
        let Err(error) = parse::<u64>(&token) else {
            panic!("nonzero huge exponent must reject");
        };
        assert!(error.len() < 80);
    }
}

#[test]
fn zero_and_long_exponent_cancellation_remain_valid() {
    for token in ["0", "-0", "-0.0", "0e10000", "-0e-10000"] {
        assert_eq!(parse::<u64>(token), Ok(0));
    }
    let exponent = "9".repeat(10_000);
    assert_eq!(parse::<u64>(&format!("-0.0e+{exponent}")), Ok(0));
    assert_eq!(parse::<u64>(&format!("0e-{exponent}")), Ok(0));
    let zeros = "0".repeat(10_000);
    assert_eq!(parse::<u64>(&format!("12{zeros}e-10000")), Ok(12));
    assert_eq!(parse::<u64>(&format!("0.{zeros}1e10001")), Ok(1));
}

#[test]
fn nonnumbers_and_invalid_json_reject() {
    for token in [
        "null",
        "true",
        "[]",
        "{}",
        "\"1\"",
        "0e+",
        "01",
        "+1",
        "1.",
        "NaN",
        "Infinity",
        r#"{"$serde_json::private::Number":"1"}"#,
        r#"{"$serde_json::private::RawValue":"1"}"#,
    ] {
        assert!(parse::<u64>(token).is_err(), "{token}");
    }
}
