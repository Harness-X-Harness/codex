//! Accept Grok JSON whole numbers for grok-build tool arguments.
//!
//! Grok emits `180000.0` for JSON Schema `number`. Serde `i64`/`u64`/`usize`/`i32`
//! reject that token. `serde_json`'s `arbitrary_precision` feature also makes
//! `deserialize_any` see dotted numbers as a map, so this helper deserializes
//! `serde_json::Number` (and `Value` only for `fork_turns` string-or-number).
//! Decimal and exponent tokens are converted from the JSON lexical form, not `f64`.

use serde::Deserialize;
use serde::Deserializer;
use serde::de::Error as DeError;
use serde_json::Number;
use serde_json::Value;

const WHOLE_NUMBER_ERROR: &str = "must be a finite whole number";

pub(crate) fn deserialize_option_whole_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Number>::deserialize(deserializer)? {
        None => Ok(None),
        Some(number) => Ok(Some(number_to_i64(number)?)),
    }
}

pub(crate) fn deserialize_whole_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    number_to_u64(Number::deserialize(deserializer)?)
}

pub(crate) fn deserialize_option_whole_usize<'de, D>(
    deserializer: D,
) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Number>::deserialize(deserializer)? {
        None => Ok(None),
        Some(number) => Ok(Some(number_to_usize(number)?)),
    }
}

pub(crate) fn deserialize_whole_i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    i32::try_from(number_to_i64(Number::deserialize(deserializer)?)?)
        .map_err(|_| DeError::custom(WHOLE_NUMBER_ERROR))
}

pub(crate) fn deserialize_optional_string_or_whole_count<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        Value::Number(number) => Ok(Some(number_to_u64(number)?.to_string())),
        _ => Err(DeError::custom(
            "fork_turns must be `none`, `all`, or a positive whole count",
        )),
    }
}

fn number_to_i64<E: DeError>(number: Number) -> Result<i64, E> {
    if let Some(value) = number.as_i64() {
        return Ok(value);
    }
    if let Some(value) = number.as_u64() {
        return i64::try_from(value).map_err(|_| E::custom(WHOLE_NUMBER_ERROR));
    }
    i64::try_from(lexical_whole_i128(&number)?).map_err(|_| E::custom(WHOLE_NUMBER_ERROR))
}

fn number_to_u64<E: DeError>(number: Number) -> Result<u64, E> {
    if let Some(value) = number.as_u64() {
        return Ok(value);
    }
    if let Some(value) = number.as_i64() {
        return u64::try_from(value).map_err(|_| E::custom(WHOLE_NUMBER_ERROR));
    }
    u64::try_from(lexical_whole_i128(&number)?).map_err(|_| E::custom(WHOLE_NUMBER_ERROR))
}

fn number_to_usize<E: DeError>(number: Number) -> Result<usize, E> {
    usize::try_from(number_to_u64(number)?).map_err(|_| E::custom(WHOLE_NUMBER_ERROR))
}

fn lexical_whole_i128<E: DeError>(number: &Number) -> Result<i128, E> {
    parse_json_whole_i128(&number.to_string()).ok_or_else(|| E::custom(WHOLE_NUMBER_ERROR))
}

fn parse_json_whole_i128(token: &str) -> Option<i128> {
    let bytes = token.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (negative, mantissa) = if bytes[0] == b'-' {
        (true, bytes.get(1..)?)
    } else {
        (false, bytes)
    };
    if mantissa.is_empty() {
        return None;
    }

    let exp_at = mantissa.iter().position(|&b| b == b'e' || b == b'E');
    let (digits_part, exponent) = match exp_at {
        Some(index) => (
            mantissa.get(..index)?,
            parse_exponent(mantissa.get(index + 1..)?)?,
        ),
        None => (mantissa, 0_i32),
    };
    if digits_part.is_empty() {
        return None;
    }

    let dot_at = digits_part.iter().position(|&b| b == b'.');
    let (int_digits, frac_digits) = match dot_at {
        Some(index) => (digits_part.get(..index)?, digits_part.get(index + 1..)?),
        None => (digits_part, &b""[..]),
    };
    if int_digits.is_empty()
        || !int_digits.iter().all(u8::is_ascii_digit)
        || !frac_digits.iter().all(u8::is_ascii_digit)
    {
        return None;
    }

    if int_digits
        .iter()
        .chain(frac_digits)
        .all(|&digit| digit == b'0')
    {
        return Some(0);
    }

    let scale = exponent.checked_sub(i32::try_from(frac_digits.len()).ok()?)?;
    if scale >= 0 {
        let scale_digits = usize::try_from(scale).ok()?;
        let significant = int_digits
            .iter()
            .chain(frac_digits)
            .skip_while(|digit| **digit == b'0')
            .count();
        if significant.checked_add(scale_digits)? > 39 {
            return None;
        }
        let mut value = parse_ascii_i128(int_digits, frac_digits, /*negative*/ false)?;
        for _ in 0..scale_digits {
            value = value.checked_mul(10)?;
        }
        return if negative {
            value.checked_neg()
        } else {
            Some(value)
        };
    }

    let drop = usize::try_from(scale.checked_neg()?)?;
    let total = int_digits.len().checked_add(frac_digits.len())?;
    if drop > total {
        return None;
    }
    let split = total - drop;
    for index in split..total {
        let digit = if index < int_digits.len() {
            *int_digits.get(index)?
        } else {
            *frac_digits.get(index - int_digits.len())?
        };
        if digit != b'0' {
            return None;
        }
    }
    if split <= int_digits.len() {
        parse_ascii_i128(&int_digits[..split], &[], negative)
    } else {
        parse_ascii_i128(
            int_digits,
            &frac_digits[..split - int_digits.len()],
            negative,
        )
    }
}

fn parse_exponent(token: &[u8]) -> Option<i32> {
    if token.is_empty() {
        return None;
    }
    let (negative, digits) = match token[0] {
        b'+' => (false, token.get(1..)?),
        b'-' => (true, token.get(1..)?),
        _ => (false, token),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0_i32;
    for digit in digits {
        value = value
            .checked_mul(10)?
            .checked_add(i32::from(digit - b'0'))?;
    }
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

fn parse_ascii_i128(left: &[u8], right: &[u8], negative: bool) -> Option<i128> {
    let mut value = 0_i128;
    for digit in left.iter().chain(right) {
        value = value
            .checked_mul(10)?
            .checked_add(i128::from(digit - b'0'))?;
    }
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

#[cfg(test)]
#[path = "json_whole_number_tests.rs"]
mod tests;
