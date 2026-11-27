use super::WaitArgs;
use crate::tools::handlers::multi_agent_wait_fields_tests as checks;

#[test]
fn timeout_whole_numbers_keep_exact_signed_values() {
    checks::timeout_whole_numbers_keep_exact_signed_values(|input| {
        serde_json::from_str::<WaitArgs>(input).map(|args| args.timeout_ms)
    });
}

#[test]
fn timeout_missing_and_null_remain_unspecified() {
    checks::timeout_missing_and_null_remain_unspecified(|input| {
        serde_json::from_str::<WaitArgs>(input).map(|args| args.timeout_ms)
    });
}

#[test]
fn timeout_rejects_fraction_range_and_nonnumber_inputs() {
    checks::timeout_rejects_fraction_range_and_nonnumber_inputs(|input| {
        serde_json::from_str::<WaitArgs>(input).map(|args| args.timeout_ms)
    });
}
