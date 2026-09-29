pub use codex_client::Provider;
pub use codex_client::RetryConfig;

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Responses-family wire dialect consumed at the API boundary.
///
/// The stock HTTP Provider remains owned by codex-client. Dialect is carried
/// separately by endpoint clients and must never be inferred from display name
/// or destination URL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ApiDialect {
    #[default]
    OpenAi,
    Grok,
}

/// Optional Grok hosted `x_search` date window.
///
/// Configured as `[model_providers.*.x_search]` with calendar `YYYY-MM-DD`
/// `from_date` / `to_date`. Other dialects ignore this field.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct XSearchProviderConfig {
    /// Start of the Grok hosted `x_search` window (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_date: Option<String>,
    /// End of the Grok hosted `x_search` window (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_date: Option<String>,
}

impl XSearchProviderConfig {
    pub fn parse_ymd(value: &str) -> Option<String> {
        let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
        (parsed.format("%Y-%m-%d").to_string() == value).then(|| value.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        for (field, value) in [("from_date", &self.from_date), ("to_date", &self.to_date)] {
            if let Some(value) = value
                && Self::parse_ymd(value).is_none()
            {
                return Err(format!(
                    "x_search.{field} `{value}` must be a calendar YYYY-MM-DD"
                ));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.from_date.is_none() && self.to_date.is_none()
    }
}

impl<'de> Deserialize<'de> for XSearchProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawXSearchProviderConfig {
            #[serde(default)]
            from_date: Option<String>,
            #[serde(default)]
            to_date: Option<String>,
        }

        let raw = RawXSearchProviderConfig::deserialize(deserializer)?;
        let config = Self {
            from_date: raw.from_date,
            to_date: raw.to_date,
        };
        config.validate().map_err(serde::de::Error::custom)?;
        Ok(config)
    }
}

pub fn is_azure_responses_provider(name: &str, base_url: Option<&str>) -> bool {
    if name.eq_ignore_ascii_case("azure") {
        true
    } else if let Some(base_url) = base_url {
        matches_azure_responses_base_url(base_url)
    } else {
        false
    }
}

fn matches_azure_responses_base_url(base_url: &str) -> bool {
    let base_url = base_url.to_ascii_lowercase();
    const AZURE_MARKERS: [&str; 6] = [
        "openai.azure.",
        "cognitiveservices.azure.",
        "aoai.azure.",
        "azure-api.",
        "azurefd.",
        "windows.net/openai",
    ];
    AZURE_MARKERS.iter().any(|marker| base_url.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_azure_responses_base_urls() {
        let positive_cases = [
            "https://foo.openai.azure.com/openai",
            "https://foo.openai.azure.us/openai/deployments/bar",
            "https://foo.cognitiveservices.azure.cn/openai",
            "https://foo.aoai.azure.com/openai",
            "https://foo.openai.azure-api.net/openai",
            "https://foo.z01.azurefd.net/",
        ];

        for base_url in positive_cases {
            assert!(
                is_azure_responses_provider("test", Some(base_url)),
                "expected {base_url} to be detected as Azure"
            );
        }

        assert!(is_azure_responses_provider(
            "Azure",
            Some("https://example.com")
        ));

        let negative_cases = [
            "https://api.openai.com/v1",
            "https://example.com/openai",
            "https://myproxy.azurewebsites.net/openai",
        ];

        for base_url in negative_cases {
            assert!(
                !is_azure_responses_provider("test", Some(base_url)),
                "expected {base_url} not to be detected as Azure"
            );
        }
    }
}
