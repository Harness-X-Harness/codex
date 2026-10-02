use crate::ProviderCapabilities;
use crate::RemoteCompactionSupport;
use crate::create_model_provider;
use codex_api::ApiDialect;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[test]
fn grok_stream_defaults_fill_only_unset_provider_policy() {
    for (idle, retries, expected_idle, expected_retries) in [
        (None, None, 60_000, 1),
        (Some(1_234), None, 1_234, 1),
        (None, Some(0), 60_000, 0),
        (Some(0), Some(7), 0, 7),
    ] {
        let configured = ModelProviderInfo {
            name: "Custom endpoint".into(),
            wire_api: WireApi::GrokResponses,
            stream_idle_timeout_ms: idle,
            stream_max_retries: retries,
            request_max_retries: Some(0),
            websocket_connect_timeout_ms: Some(123),
            ..ModelProviderInfo::default()
        };
        let provider = create_model_provider(configured.clone(), /*auth_manager*/ None);
        assert_eq!(
            (provider.info(), provider.api_dialect()),
            (
                &ModelProviderInfo {
                    stream_idle_timeout_ms: Some(expected_idle),
                    stream_max_retries: Some(expected_retries),
                    ..configured
                },
                ApiDialect::Grok
            )
        );
        assert_eq!(
            (
                provider.info().stream_idle_timeout(),
                provider.info().stream_max_retries()
            ),
            (Duration::from_millis(expected_idle), expected_retries)
        );
    }
}

#[test]
fn responses_stream_policy_ignores_grok_name_and_destination() {
    let configured = ModelProviderInfo {
        name: "Grok".into(),
        base_url: Some("https://api.x.ai/v1".into()),
        ..ModelProviderInfo::default()
    };
    let provider = create_model_provider(configured.clone(), /*auth_manager*/ None);
    assert_eq!(
        (provider.info(), provider.api_dialect()),
        (&configured, ApiDialect::OpenAi)
    );
    assert_eq!(
        (
            provider.info().stream_idle_timeout(),
            provider.info().stream_max_retries()
        ),
        (Duration::from_millis(300_000), 5)
    );
}

#[test]
fn compaction_capability_respects_explicit_dialect_before_provider_aliases() {
    for (name, base_url, responses_support) in [
        (
            "Grok",
            "https://api.x.ai/v1",
            RemoteCompactionSupport::Unsupported,
        ),
        (
            "OpenAI",
            "https://example.test/v1",
            RemoteCompactionSupport::V2,
        ),
        (
            "Azure",
            "https://example.test/v1",
            RemoteCompactionSupport::V2,
        ),
        (
            "Custom",
            "https://example.openai.azure.com/openai/v1",
            RemoteCompactionSupport::V2,
        ),
    ] {
        for (wire_api, dialect, remote_compaction) in [
            (
                WireApi::GrokResponses,
                ApiDialect::Grok,
                RemoteCompactionSupport::Unsupported,
            ),
            (WireApi::Responses, ApiDialect::OpenAi, responses_support),
        ] {
            let provider = create_model_provider(
                ModelProviderInfo {
                    name: name.to_string(),
                    base_url: Some(base_url.to_string()),
                    wire_api,
                    ..ModelProviderInfo::default()
                },
                /*auth_manager*/ None,
            );
            assert_eq!(
                (provider.api_dialect(), provider.capabilities()),
                (
                    dialect,
                    ProviderCapabilities {
                        remote_compaction,
                        ..ProviderCapabilities::default()
                    }
                ),
                "{name} at {base_url}"
            );
        }
    }
}
