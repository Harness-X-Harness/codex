use crate::common::ResponsesApiRequest;
use codex_client::Request;
use codex_client::RequestCompression;
use codex_client::RetryOn;
use codex_client::RetryPolicy;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use http::Method;
use http::header::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

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

/// Internal Responses wire shape selected from the resolved API provider.
///
/// This is runtime-only API-boundary state. It does not change durable Codex
/// history or introduce another serialized provider selector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ResponsesDialect {
    #[default]
    OpenAi,
    Grok,
}

impl ResponsesDialect {
    pub(crate) fn for_provider(provider: &Provider) -> Self {
        if provider.name.eq_ignore_ascii_case("Grok") {
            Self::Grok
        } else {
            Self::OpenAi
        }
    }

    pub(crate) fn project_request(
        self,
        request: &ResponsesApiRequest,
    ) -> serde_json::Result<Value> {
        if self == Self::OpenAi {
            return serde_json::to_value(request);
        }

        let mut request = request.clone();
        for item in &mut request.input {
            let projected_item = match item {
                ResponseItem::AgentMessage {
                    id,
                    content,
                    internal_chat_message_metadata_passthrough,
                    ..
                } => plaintext_agent_message_content(content).map(|text| ResponseItem::Message {
                    id: id.clone(),
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText { text }],
                    phase: None,
                    internal_chat_message_metadata_passthrough:
                        internal_chat_message_metadata_passthrough.clone(),
                }),
                ResponseItem::FunctionCallOutput {
                    id,
                    call_id: None,
                    name: Some(name),
                    output,
                    internal_chat_message_metadata_passthrough,
                    ..
                } if !name.is_empty() => output.text_content().map(|text| ResponseItem::Message {
                    id: id.clone(),
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: text.to_string(),
                    }],
                    phase: None,
                    internal_chat_message_metadata_passthrough:
                        internal_chat_message_metadata_passthrough.clone(),
                }),
                _ => None,
            };
            if let Some(projected_item) = projected_item {
                *item = projected_item;
                continue;
            }

            // Grok accepts encrypted reasoning replay, but not `content: null`.
            // `Some(Vec::new())` is intentionally omitted by ResponseItem serde.
            if let ResponseItem::Reasoning {
                content,
                encrypted_content: Some(_),
                ..
            } = item
                && content.is_none()
            {
                *content = Some(Vec::new());
            }
        }

        for item in &request.input {
            match item {
                ResponseItem::AgentMessage { .. } => {
                    return Err(<serde_json::Error as serde::ser::Error>::custom(
                        "Grok cannot replay unsupported encrypted collaboration history",
                    ));
                }
                ResponseItem::FunctionCallOutput { call_id, .. }
                    if call_id.as_deref().is_none_or(str::is_empty) =>
                {
                    return Err(<serde_json::Error as serde::ser::Error>::custom(
                        "Grok cannot replay function_call_output history without call_id",
                    ));
                }
                _ => {}
            }
        }

        let mut value = serde_json::to_value(&request)?;
        if let Some(object) = value.as_object_mut() {
            // Grok's verified no-tool request omits the tool-control trio entirely.
            // Non-empty tool projection is owned by #206.
            let has_no_tools = match object.get("tools") {
                None => true,
                Some(tools) => tools.as_array().is_some_and(Vec::is_empty),
            };
            if has_no_tools {
                object.remove("tools");
                object.remove("tool_choice");
                object.remove("parallel_tool_calls");
            }
        }
        Ok(value)
    }
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
#[path = "provider_grok_tests.rs"]
mod grok_tests;

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
