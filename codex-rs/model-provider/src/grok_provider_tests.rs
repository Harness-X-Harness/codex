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

/// Any access fails this test, including auth refresh and ETag revalidation.
#[derive(Debug)]
struct UnusedCache;

impl codex_models_manager::cache::ModelsCache for UnusedCache {
    fn load<'a>(
        &'a self,
        _client_version: &'a str,
    ) -> codex_models_manager::cache::ModelsCacheFuture<
        'a,
        Result<
            Option<codex_models_manager::cache::ModelsCacheEntry>,
            codex_models_manager::cache::ModelsCacheError,
        >,
    > {
        panic!("authoritative catalog must not read the supplied cache")
    }

    fn store<'a>(
        &'a self,
        _entry: &'a codex_models_manager::cache::ModelsCacheEntry,
    ) -> codex_models_manager::cache::ModelsCacheFuture<
        'a,
        Result<(), codex_models_manager::cache::ModelsCacheError>,
    > {
        panic!("authoritative catalog must not write the supplied cache")
    }

    fn refresh_ttl<'a>(
        &'a self,
        _client_version: &'a str,
        _identity: &'a str,
        _etag: &'a str,
    ) -> codex_models_manager::cache::ModelsCacheFuture<
        'a,
        Result<(), codex_models_manager::cache::ModelsCacheError>,
    > {
        panic!("authoritative catalog must not renew the supplied cache")
    }
}

#[tokio::test]
async fn grok_catalog_replaces_remote_bundled_and_cached_models_in_every_constructor() {
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use codex_models_manager::ModelsManagerConfig;
    use codex_models_manager::manager::RefreshStrategy;
    use codex_protocol::openai_models::ModelsResponse;
    use std::sync::Arc;

    let server = wiremock::MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let cache_path = home.path().join("models_cache.json");
    let foreign_cache = serde_json::to_vec(&crate::test_support::models_cache_entry(
        &ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth*/ None,
        codex_models_manager::bundled_models_response()
            .unwrap()
            .models,
    ))
    .unwrap();
    std::fs::write(&cache_path, &foreign_cache).unwrap();
    let fallback = crate::grok_catalog::static_model_catalog();
    let mut configured_model = fallback.models[0].clone();
    configured_model.slug = "configured-only".into();
    let configured = ModelsResponse {
        models: vec![configured_model],
    };
    let http = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    for name in [
        "Grok",
        "OpenAI",
        "arbitrary alias",
        "Amazon Bedrock",
        "Amazon Bedrock Runtime",
    ] {
        let provider = create_model_provider(
            ModelProviderInfo {
                name: name.into(),
                base_url: Some(server.uri()),
                model_catalog_url: Some(format!("{}/models", server.uri()).into()),
                experimental_bearer_token: Some("fixture-key".into()),
                wire_api: WireApi::GrokResponses,
                ..Default::default()
            },
            /*auth_manager*/ None,
        );
        for supplied in [None, Some(configured.clone())] {
            let expected = supplied.clone().unwrap_or_else(|| fallback.clone());
            for manager in [
                provider.models_manager(home.path().to_path_buf(), supplied.clone()),
                provider.models_manager_without_cache(supplied.clone()),
                provider.models_manager_with_cache(supplied.clone(), Arc::new(UnusedCache)),
            ] {
                manager.set_api_key_model_discovery_enabled(true);
                for strategy in [
                    RefreshStrategy::Offline,
                    RefreshStrategy::OnlineIfUncached,
                    RefreshStrategy::Online,
                ] {
                    assert_eq!(
                        manager.raw_model_catalog(strategy, http.clone()).await,
                        expected
                    );
                }
                manager.refresh_after_auth_change(http.clone()).await;
                manager
                    .refresh_if_new_etag("changed".into(), http.clone())
                    .await;
                assert_eq!(manager.get_remote_models().await, expected.models);
                assert_eq!(manager.try_get_remote_models().unwrap(), expected.models);
                let models = manager
                    .list_models(RefreshStrategy::Online, http.clone())
                    .await;
                assert_eq!(models.len(), 1);
                assert_eq!(models[0].model, expected.models[0].slug);
                assert!(models[0].is_default);
                assert_eq!(
                    manager
                        .get_default_model(
                            &None,
                            /*allow_provider_model_fallback*/ false,
                            RefreshStrategy::Online,
                            http.clone()
                        )
                        .await,
                    expected.models[0].slug
                );
                let unlisted = Some("unlisted-model".into());
                assert_eq!(
                    manager
                        .get_default_model(
                            &unlisted,
                            /*allow_provider_model_fallback*/ false,
                            RefreshStrategy::Offline,
                            http.clone()
                        )
                        .await,
                    "unlisted-model"
                );
                assert_eq!(
                    manager
                        .get_model_info("unlisted-model", &ModelsManagerConfig::default())
                        .await
                        .slug,
                    "unlisted-model"
                );
            }
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(std::fs::read(cache_path).unwrap(), foreign_cache);
}

#[tokio::test]
async fn grok_name_and_url_do_not_select_catalog_or_internal_model_policy() {
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use codex_login::AuthManager;
    use codex_login::CodexAuth;
    use codex_models_manager::manager::RefreshStrategy;

    let http = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    for wire_api in [WireApi::Responses, WireApi::GrokResponses] {
        for auth in [None, Some(CodexAuth::from_api_key("fixture-key"))] {
            let grok = wire_api == WireApi::GrokResponses;
            let provider = create_model_provider(
                ModelProviderInfo {
                    name: "Grok".into(),
                    base_url: Some("https://grok.example.test/v1".into()),
                    wire_api,
                    requires_openai_auth: true,
                    ..Default::default()
                },
                auth.clone().map(AuthManager::from_auth_for_testing),
            );
            let expected_review = if grok {
                "grok-4.6"
            } else if auth.is_some() {
                "gpt-5.6-luna"
            } else {
                "codex-auto-review"
            };
            assert_eq!(
                (
                    provider.approval_review_preferred_model(),
                    provider.memory_extraction_preferred_model(),
                    provider.memory_consolidation_preferred_model()
                ),
                (
                    expected_review,
                    if grok { "grok-4.6" } else { "gpt-5.6-luna" },
                    if grok { "grok-4.6" } else { "gpt-5.6-terra" }
                )
            );
            let actual = provider
                .models_manager_without_cache(/*config_model_catalog*/ None)
                .raw_model_catalog(RefreshStrategy::Offline, http.clone())
                .await;
            assert_eq!(
                actual,
                if grok {
                    crate::grok_catalog::static_model_catalog()
                } else {
                    codex_models_manager::bundled_models_response().unwrap()
                }
            );
        }
    }
}

#[test]
fn stock_bedrock_names_keep_their_provider_factory_branch() {
    for name in ["Amazon Bedrock", "Amazon Bedrock Runtime"] {
        let mut info = ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None);
        info.name = name.into();
        let provider = create_model_provider(info.clone(), /*auth_manager*/ None);
        assert_eq!(provider.info(), &info);
        assert_eq!(provider.api_dialect(), ApiDialect::OpenAi);
        assert_eq!(
            provider.account_state(),
            Ok(crate::ProviderAccountState {
                account: Some(codex_protocol::account::ProviderAccount::AmazonBedrock {
                    uses_codex_managed_credentials: false
                }),
                requires_openai_auth: false,
            })
        );
    }
}
