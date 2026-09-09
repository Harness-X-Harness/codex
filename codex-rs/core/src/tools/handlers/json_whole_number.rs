//! Accept Grok JSON whole numbers for grok-build tool arguments.
//!
//! Grok emits `180000.0` for JSON Schema `number`. Serde `i64`/`u64`/`usize`/`i32`
//! reject that token. `serde_json`'s `arbitrary_precision` feature also makes
//! `deserialize_any` see dotted numbers as a map, so this helper deserializes
//! `serde_json::Number` (and `Value` only for `fork_turns` string-or-number).

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
    f64_to_i64(number_as_f64(number)?)
}

fn number_to_u64<E: DeError>(number: Number) -> Result<u64, E> {
    if let Some(value) = number.as_u64() {
        return Ok(value);
    }
    if let Some(value) = number.as_i64() {
        return u64::try_from(value).map_err(|_| E::custom(WHOLE_NUMBER_ERROR));
    }
    f64_to_u64(number_as_f64(number)?)
}

fn number_to_usize<E: DeError>(number: Number) -> Result<usize, E> {
    usize::try_from(number_to_u64(number)?).map_err(|_| E::custom(WHOLE_NUMBER_ERROR))
}

fn number_as_f64<E: DeError>(number: Number) -> Result<f64, E> {
    number.as_f64().ok_or_else(|| E::custom(WHOLE_NUMBER_ERROR))
}

fn f64_to_i64<E: DeError>(value: f64) -> Result<i64, E> {
    let value = reject_non_whole_float(value)?;
    if value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(E::custom(WHOLE_NUMBER_ERROR));
    }
    let converted = value as i64;
    if converted as f64 != value {
        return Err(E::custom(WHOLE_NUMBER_ERROR));
    }
    Ok(converted)
}

fn f64_to_u64<E: DeError>(value: f64) -> Result<u64, E> {
    let value = reject_non_whole_float(value)?;
    if value < 0.0 || value > u64::MAX as f64 {
        return Err(E::custom(WHOLE_NUMBER_ERROR));
    }
    let converted = value as u64;
    if converted as f64 != value {
        return Err(E::custom(WHOLE_NUMBER_ERROR));
    }
    Ok(converted)
}

fn reject_non_whole_float<E: DeError>(value: f64) -> Result<f64, E> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(E::custom(WHOLE_NUMBER_ERROR));
    }
    Ok(value)
}
