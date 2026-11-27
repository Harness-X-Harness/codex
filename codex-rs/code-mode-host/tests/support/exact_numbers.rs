use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use serde_json::Number;
use serde_json::Value;

// Compare mathematical values without imposing an integer target range or a JSON spelling.
fn normalize(number: &Number) -> Result<(i8, String, i128), String> {
    let text = number.as_str();
    let (sign, unsigned) = text.strip_prefix('-').map_or((1, text), |rest| (-1, rest));
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let fraction_len = mantissa.split_once('.').map_or(0, |(_, tail)| tail.len());
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.is_empty() {
        return Ok((1, "0".to_string(), 0));
    }
    let trailing_zeroes = digits.len() - digits.trim_end_matches('0').len();
    let scale = exponent
        .parse::<i128>()
        .ok()
        .and_then(|exponent| exponent.checked_sub(i128::try_from(fraction_len).ok()?))
        .and_then(|scale| scale.checked_add(i128::try_from(trailing_zeroes).ok()?))
        .ok_or_else(|| "numeric scale exceeds the finite fixture oracle".to_string())?;
    Ok((sign, significant.to_string(), scale))
}

pub(crate) fn assert_exact_numbers(input: &Value) {
    let actual: BTreeMap<_, _> = input
        .as_object()
        .unwrap_or_else(|| panic!("numeric fixture object"))
        .iter()
        .map(|(name, value)| {
            let Value::Number(number) = value else {
                panic!("{name} must remain a JSON number: {value:?}");
            };
            (
                name.as_str(),
                normalize(number).unwrap_or_else(|error| panic!("{error}")),
            )
        })
        .collect();
    assert_eq!(
        actual,
        BTreeMap::from([
            ("beyond_binary64", (1, "9007199254740993".to_string(), 0)),
            ("fraction", (1, "10000000000000001".to_string(), -16)),
            ("u64_boundary", (1, "18446744073709551615".to_string(), 0)),
        ])
    );
}

#[test]
fn numeric_oracle_accepts_equivalent_spellings_and_distinguishes_rounding() {
    for (text, expected) in [
        ("9007199254740993", (1, "9007199254740993", 0)),
        ("9007199254740993.0", (1, "9007199254740993", 0)),
        ("9.007199254740993e15", (1, "9007199254740993", 0)),
        ("184467440737095516150e-1", (1, "18446744073709551615", 0)),
        ("18446744073709551615", (1, "18446744073709551615", 0)),
        ("18446744073709551615.0", (1, "18446744073709551615", 0)),
        ("1.0000000000000001", (1, "10000000000000001", -16)),
        ("10000000000000001e-16", (1, "10000000000000001", -16)),
        ("9007199254740992", (1, "9007199254740992", 0)),
        ("1", (1, "1", 0)),
        ("-1.20e2", (-1, "12", 1)),
        ("-0.000e20", (1, "0", 0)),
    ] {
        let actual = normalize(&text.parse::<Number>().expect("valid number"));
        assert_eq!(
            actual,
            Ok((expected.0, expected.1.to_string(), expected.2)),
            "{text}"
        );
    }
    for (rounded, exact) in [
        ("9007199254740992", "9007199254740993.0"),
        ("1", "1.0000000000000001"),
    ] {
        assert_ne!(
            normalize(&rounded.parse().expect("rounded number")),
            normalize(&exact.parse().expect("exact number"))
        );
    }
}
