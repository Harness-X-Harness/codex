//! Shared Grok policy types, independent of transport and runtime ownership.

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Provider defaults for the inclusive X Search date window.
///
/// Explicit request dates override defaults independently. Empty options do not
/// disable X Search: a nonempty Grok tool plan always includes it exactly once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrokXSearchOptions {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
}

impl GrokXSearchOptions {
    /// Reject malformed calendar dates and reversed ranges without normalizing them.
    pub fn validate(&self) -> Result<(), String> {
        for (field, date) in [
            ("from_date", self.from_date.as_deref()),
            ("to_date", self.to_date.as_deref()),
        ] {
            if let Some(date) = date {
                let bytes = date.as_bytes();
                if bytes.len() != 10
                    || bytes.iter().enumerate().any(|(index, byte)| {
                        if matches!(index, 4 | 7) {
                            *byte != b'-'
                        } else {
                            !byte.is_ascii_digit()
                        }
                    })
                    || NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
                {
                    return Err(format!(
                        "Grok x_search.{field} must be a calendar YYYY-MM-DD"
                    ));
                }
            }
        }
        if let (Some(from), Some(to)) = (&self.from_date, &self.to_date)
            && from > to
        {
            return Err("Grok x_search.from_date must not be after to_date".into());
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for GrokXSearchOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Options {
            from_date: Option<String>,
            to_date: Option<String>,
        }
        let Options { from_date, to_date } = Options::deserialize(deserializer)?;
        let options = Self { from_date, to_date };
        options.validate().map_err(serde::de::Error::custom)?;
        Ok(options)
    }
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod tests;
