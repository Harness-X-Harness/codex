use super::ConfigToml;
use super::resolve_web_search_config;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::config_types::WebSearchConfig;
use codex_protocol::config_types::WebSearchFilters;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn grok_hosted_web_search_config_preserves_excluded_domains() {
    let config: ConfigToml = toml::from_str(
        r#"
[tools.web_search]
excluded_domains = ["example.com"]
"#,
    )
    .expect("parse web search configuration");
    let provider = ModelProviderInfo {
        wire_api: WireApi::GrokResponses,
        ..Default::default()
    };

    assert_eq!(
        resolve_web_search_config(
            config
                .tools
                .as_ref()
                .and_then(|tools| tools.web_search.as_ref()),
            &provider,
        )
        .expect("resolve Grok hosted search"),
        Some(WebSearchConfig {
            filters: Some(WebSearchFilters {
                allowed_domains: None,
                excluded_domains: Some(vec!["example.com".to_string()]),
            }),
            ..Default::default()
        })
    );
}

#[test]
fn excluded_domains_fail_closed_without_grok_hosted_search() {
    let config: ConfigToml = toml::from_str(
        r#"
[tools.web_search]
excluded_domains = ["example.com"]
"#,
    )
    .expect("parse web search configuration");
    for provider in [
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            supports_standalone_web_search: true,
            ..Default::default()
        },
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            supports_standalone_web_search: false,
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        },
        ModelProviderInfo {
            wire_api: WireApi::GrokResponses,
            http_headers: Some(HashMap::from([(
                "x-openai-actor-authorization".to_string(),
                "test-actor".to_string(),
            )])),
            ..Default::default()
        },
    ] {
        let error = resolve_web_search_config(
            config
                .tools
                .as_ref()
                .and_then(|tools| tools.web_search.as_ref()),
            &provider,
        )
        .expect_err("unsupported exclusion policy must not be dropped");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "tools.web_search.excluded_domains requires a Grok hosted-search provider without standalone web search"
        );
    }
}

#[test]
fn grok_web_search_config_rejects_conflicting_nonempty_domain_policies() {
    let config: ConfigToml = toml::from_str(
        r#"
[tools.web_search]
allowed_domains = ["allowed.example"]
excluded_domains = ["excluded.example"]
"#,
    )
    .expect("parse web search configuration");
    let provider = ModelProviderInfo {
        wire_api: WireApi::GrokResponses,
        ..Default::default()
    };
    let error = resolve_web_search_config(
        config
            .tools
            .as_ref()
            .and_then(|tools| tools.web_search.as_ref()),
        &provider,
    )
    .expect_err("conflicting policy must fail closed");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "tools.web_search.allowed_domains and tools.web_search.excluded_domains are mutually exclusive"
    );
}

#[test]
fn empty_excluded_domains_preserve_stock_allowed_domain_config() {
    let config: ConfigToml = toml::from_str(
        r#"
[tools.web_search]
allowed_domains = ["example.com"]
excluded_domains = []
"#,
    )
    .expect("parse web search configuration");
    let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);

    assert_eq!(
        resolve_web_search_config(
            config
                .tools
                .as_ref()
                .and_then(|tools| tools.web_search.as_ref()),
            &provider,
        )
        .expect("an empty exclusion list imposes no restriction"),
        Some(WebSearchConfig {
            filters: Some(WebSearchFilters {
                allowed_domains: Some(vec!["example.com".to_string()]),
                excluded_domains: None,
            }),
            ..Default::default()
        })
    );
}
