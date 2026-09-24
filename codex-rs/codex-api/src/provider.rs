use chrono::NaiveDate;
use codex_client::Request;
use codex_client::RequestCompression;
use codex_client::RetryOn;
use codex_client::RetryPolicy;
use http::Method;
use http::header::HeaderMap;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

/// Explicit Responses/Images wire dialect.
///
/// Mapped exactly once from `WireApi` in `ModelProviderInfo::to_api_provider()`.
/// Runtime request/stream/image projection must consume this value instead of
/// inferring dialect from provider display name or hostname.
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
    /// Parse a real calendar day in exactly `YYYY-MM-DD` form.
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

/// High-level retry configuration for a provider.
///
/// This is converted into a `RetryPolicy` used by `codex-client` to drive
/// transport-level retries for both unary and streaming calls.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u64,
    pub base_delay: Duration,
    pub retry_429: bool,
    pub retry_5xx: bool,
    pub retry_transport: bool,
}

impl RetryConfig {
    pub fn to_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_attempts: self.max_attempts,
            base_delay: self.base_delay,
            retry_on: RetryOn {
                retry_429: self.retry_429,
                retry_5xx: self.retry_5xx,
                retry_transport: self.retry_transport,
            },
        }
    }
}

/// HTTP endpoint configuration used to talk to a concrete API deployment.
///
/// Encapsulates base URL, default headers, query params, retry policy, and
/// stream idle timeout, plus helper methods for building requests.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub query_params: Option<HashMap<String, String>>,
    pub headers: HeaderMap,
    pub retry: RetryConfig,
    pub stream_idle_timeout: Duration,
    /// Wire dialect mapped from `WireApi`. Routing must not mutate this field.
    pub dialect: ApiDialect,
    /// Grok hosted `x_search` window copied from provider config. Ignored by
    /// other dialects.
    pub x_search: Option<XSearchProviderConfig>,
}

impl Provider {
    pub fn url_for_path(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        let mut url = if path.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{path}")
        };

        if let Some(params) = &self.query_params
            && !params.is_empty()
        {
            let qs = params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            url.push('?');
            url.push_str(&qs);
        }

        url
    }

    pub fn build_request(&self, method: Method, path: &str) -> Request {
        Request {
            method,
            url: self.url_for_path(path),
            headers: self.headers.clone(),
            body: None,
            compression: RequestCompression::None,
            timeout: None,
            response_body_limit_bytes: None,
        }
    }

    pub fn websocket_url_for_path(&self, path: &str) -> Result<Url, url::ParseError> {
        let mut url = Url::parse(&self.url_for_path(path))?;

        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            "ws" | "wss" => return Ok(url),
            _ => return Ok(url),
        };
        let _ = url.set_scheme(scheme);
        Ok(url)
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

    #[test]
    fn api_dialect_is_stored_explicitly_and_not_inferred_from_name_or_host() {
        let provider = Provider {
            name: "Grok".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            query_params: None,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(1),
            dialect: ApiDialect::OpenAi,
            x_search: None,
        };

        assert_eq!(provider.dialect, ApiDialect::OpenAi);
        assert_eq!(provider.name, "Grok");
        assert_eq!(provider.base_url, "https://api.x.ai/v1");
    }
}
