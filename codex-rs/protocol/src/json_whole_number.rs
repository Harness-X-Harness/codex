//! Exact JSON-number adapters for the existing i32, i64, u64, and usize tool fields.
//! Decimal and exponent spellings retain integer precision without an f64 step.

use serde::Deserialize;
use serde::Deserializer;
use serde::de::Error;
use serde_json::value::RawValue;

/// Deserialize a whole JSON number into a supported integer target.
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<i128>,
{
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    from_raw(&raw).map_err(D::Error::custom)
}

/// Deserialize an optional supported integer, preserving explicit JSON null.
/// Missing-field defaults remain the containing type's responsibility.
pub fn deserialize_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<i128>,
{
    Option::<Box<RawValue>>::deserialize(deserializer)?
        .map(|raw| from_raw(&raw).map_err(D::Error::custom))
        .transpose()
}

// RawValue validates JSON syntax before this converter sees the token. Keep the
// arithmetic bounded by the selected i32/i64/u64/usize targets, not its exponent.
fn from_raw<T: TryFrom<i128>>(raw: &RawValue) -> Result<T, &'static str> {
    const RANGE_ERROR: &str = "expected an in-range whole number";
    let token = raw.get();
    if !matches!(token.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return Err("expected a JSON number");
    }
    let negative = token.starts_with('-');
    let unsigned = token.trim_start_matches('-');
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let Some(first) = mantissa
        .bytes()
        .position(|byte| matches!(byte, b'1'..=b'9'))
    else {
        return T::try_from(0).map_err(|_| RANGE_ERROR);
    };
    let last = mantissa
        .bytes()
        .rposition(|byte| matches!(byte, b'1'..=b'9'))
        .ok_or(RANGE_ERROR)?;
    let significant = &mantissa[first..=last];
    let digits = significant.bytes().filter(u8::is_ascii_digit).count();
    if digits > 20 {
        return Err(RANGE_ERROR);
    }
    let fraction = mantissa.split_once('.').map_or(0, |(_, part)| part.len());
    let trailing = mantissa[last + 1..]
        .bytes()
        .filter(u8::is_ascii_digit)
        .count();

    // A larger magnitude cannot cancel the mantissa's fractional/trailing
    // digits into a nonzero integer of at most 20 digits. usize fits in i128
    // on supported targets, including during the bounded multiply below.
    let limit = token.len() as i128 + 21;
    let mut scale = 0_i128;
    for byte in exponent.bytes().filter(u8::is_ascii_digit) {
        scale = (scale * 10 + i128::from(byte - b'0')).min(limit);
    }
    if exponent.starts_with('-') {
        scale = -scale;
    }
    scale += trailing as i128 - fraction as i128;
    if !(0..=20 - digits as i128).contains(&scale) {
        return Err(RANGE_ERROR);
    }

    let mut value = 0_i128;
    for byte in significant.bytes().filter(u8::is_ascii_digit) {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(byte - b'0')))
            .ok_or(RANGE_ERROR)?;
    }
    for _ in 0..scale {
        value = value.checked_mul(10).ok_or(RANGE_ERROR)?;
    }
    if negative {
        value = -value;
    }
    T::try_from(value).map_err(|_| RANGE_ERROR)
}

#[cfg(test)]
#[path = "json_whole_number_tests.rs"]
mod tests;
