use super::ExecCommandArgs;
use super::parse_arguments;
use pretty_assertions::assert_eq;

#[test]
fn exec_numeric_fields_preserve_exact_values_and_defaults() {
    let args: ExecCommandArgs = parse_arguments(
        r#"{"cmd":"echo ok","yield_time_ms":184467440737095516150e-1,"timeout_ms":9007199254740993.0,"max_output_tokens":1.2e2,"tty":true,"login":false,"ignored":1}"#,
    )
    .unwrap();
    assert_eq!(
        (
            args.yield_time_ms,
            args.timeout_ms,
            args.max_output_tokens,
            args.tty,
            args.login,
            args.cmd
        ),
        (
            u64::MAX,
            Some(9_007_199_254_740_993),
            Some(120),
            true,
            Some(false),
            "echo ok".to_owned()
        )
    );
    for json in [
        r#"{"cmd":"ok"}"#,
        r#"{"cmd":"ok","timeout_ms":null,"max_output_tokens":null}"#,
    ] {
        let args: ExecCommandArgs = parse_arguments(json).unwrap();
        assert_eq!(
            (args.yield_time_ms, args.timeout_ms, args.max_output_tokens),
            (10_000, None, None)
        );
    }
}

#[test]
fn exec_numeric_fields_reject_fractions_wrong_kinds_and_null_scalars() {
    for field in ["yield_time_ms", "timeout_ms", "max_output_tokens"] {
        for token in [
            "1.0000000000000001",
            "9007199254740993.1",
            "-1",
            "18446744073709551616",
            "\"1\"",
            "true",
            "[]",
            "{}",
            r#"{"$serde_json::private::Number":"1"}"#,
            r#"{"$serde_json::private::RawValue":"1"}"#,
        ] {
            assert!(
                parse_arguments::<ExecCommandArgs>(&format!(
                    r#"{{"cmd":"ok","{field}":{token}}}"#
                ))
                .is_err(),
                "{field}={token}"
            );
        }
    }
    for json in [
        r#"{"cmd":"ok","yield_time_ms":null}"#,
        r#"{"cmd":"ok","yield_time_ms":01}"#,
        r#"{"cmd":"ok","yield_time_ms":0e+}"#,
        r#"{"cmd":1}"#,
        r#"{"cmd":"ok","tty":1}"#,
        r#"{"cmd":"ok","login":"true"}"#,
    ] {
        assert!(parse_arguments::<ExecCommandArgs>(json).is_err(), "{json}");
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn exec_numeric_output_limit_keeps_bits_above_binary64() {
    let args: ExecCommandArgs =
        parse_arguments(r#"{"cmd":"ok","max_output_tokens":9.007199254740993e15}"#).unwrap();
    assert_eq!(args.max_output_tokens, Some(9_007_199_254_740_993));
}
