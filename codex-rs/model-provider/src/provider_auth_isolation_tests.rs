//! Provider credentials must not borrow an unrelated OpenAI login's lifecycle.

use super::*;
use codex_protocol::config_types::ModelProviderAuthInfo;
use http::HeaderMap;
use http::HeaderValue;
use http::header::AUTHORIZATION;
use pretty_assertions::assert_eq;
use std::num::NonZeroU64;

#[test]
fn explicit_provider_key_does_not_require_login_or_project_an_ambient_account() {
    for wire_api in [WireApi::Responses, WireApi::GrokResponses] {
        for login in [
            None,
            Some(CodexAuth::from_api_key("synthetic-unrelated-openai-key")),
            Some(CodexAuth::create_dummy_chatgpt_auth_for_testing()),
        ] {
            let provider = create_model_provider(
                ModelProviderInfo {
                    name: "Independent provider".into(),
                    base_url: Some("https://provider.example.test/v1".into()),
                    wire_api,
                    requires_openai_auth: true,
                    experimental_bearer_token: Some("synthetic-provider-token".into()),
                    ..Default::default()
                },
                login.map(AuthManager::from_auth_for_testing),
            );
            assert_eq!(
                provider.account_state(),
                Ok(ProviderAccountState {
                    account: None,
                    requires_openai_auth: false,
                })
            );
        }
    }
}

#[tokio::test]
async fn explicit_provider_credentials_isolate_auth_recovery_attestation_and_catalog() {
    let login = CodexAuth::create_dummy_chatgpt_auth_for_testing();
    let credentials = AuthManager::from_auth_for_testing(login.clone());
    let openai = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(Arc::clone(&credentials)),
    );
    let expected_openai_headers = crate::auth::auth_provider_from_auth(&login)
        .to_auth_headers()
        .unwrap();
    assert!(openai.supports_attestation());
    assert!(Arc::ptr_eq(&openai.auth_manager().unwrap(), &credentials));
    assert!(
        openai
            .auth_manager()
            .unwrap()
            .unauthorized_recovery()
            .has_next()
    );

    for wire_api in [WireApi::Responses, WireApi::GrokResponses] {
        for (requires_openai_auth, token) in [
            (false, None),
            (false, Some("synthetic-provider-token")),
            (true, Some("synthetic-provider-token")),
        ] {
            let provider = create_model_provider(
                ModelProviderInfo {
                    // Eligibility follows credential selection, not the name or dialect.
                    name: "OpenAI".into(),
                    base_url: Some("https://provider.example.test/v1".into()),
                    wire_api,
                    requires_openai_auth,
                    experimental_bearer_token: token.map(Into::into),
                    ..Default::default()
                },
                Some(Arc::clone(&credentials)),
            );
            assert!(provider.auth_manager().is_none());
            assert!(provider.auth().await.is_none());
            assert!(!provider.supports_attestation());
            assert_eq!(
                provider.account_state(),
                Ok(ProviderAccountState {
                    account: None,
                    requires_openai_auth: false,
                })
            );
            assert_eq!(
                provider.recover_from_unauthorized().await.unwrap(),
                ProviderUnauthorizedRecovery::NotConfigured
            );
            assert!(
                provider
                    .models_manager_without_cache(/*config_model_catalog*/ None)
                    .auth_manager()
                    .is_none()
            );

            let (openai_auth, provider_auth) = tokio::join!(openai.api_auth(), provider.api_auth());
            assert_eq!(
                openai_auth.unwrap().to_auth_headers().unwrap(),
                expected_openai_headers
            );
            let expected_provider_headers = token.map_or_else(HeaderMap::new, |token| {
                HeaderMap::from_iter([(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
                )])
            });
            assert_eq!(
                provider_auth.unwrap().to_auth_headers().unwrap(),
                expected_provider_headers
            );
            assert_eq!(credentials.auth_cached(), Some(login.clone()));
        }
    }
}

#[tokio::test]
async fn explicit_key_preserves_default_openai_routing_without_borrowing_login_auth() {
    let credentials =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_chatgpt_auth_for_testing());
    for (wire_api, requires_openai_auth, base_url, expected_url) in [
        (
            WireApi::Responses,
            true,
            None,
            codex_model_provider_info::CHATGPT_CODEX_BASE_URL,
        ),
        (WireApi::Responses, false, None, "https://api.openai.com/v1"),
        (
            WireApi::GrokResponses,
            true,
            None,
            "https://api.openai.com/v1",
        ),
        (
            WireApi::Responses,
            true,
            Some("https://provider.example.test/v1"),
            "https://provider.example.test/v1",
        ),
    ] {
        let provider = create_model_provider(
            ModelProviderInfo {
                base_url: base_url.map(str::to_owned),
                experimental_bearer_token: Some("synthetic-provider-token".into()),
                wire_api,
                requires_openai_auth,
                supports_websockets: false,
                ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
            },
            Some(Arc::clone(&credentials)),
        );
        assert!(provider.auth_manager().is_none());
        assert!(provider.auth().await.is_none());
        assert!(!provider.supports_attestation());
        assert_eq!(
            provider.api_provider().await.unwrap().base_url,
            expected_url
        );
        let (route, auth) = provider.api_provider_and_auth().await.unwrap();
        assert_eq!(route.base_url, expected_url);
        assert_eq!(
            auth.to_auth_headers().unwrap(),
            HeaderMap::from_iter([(
                AUTHORIZATION,
                HeaderValue::from_static("Bearer synthetic-provider-token"),
            )])
        );
        assert!(
            provider
                .models_manager_without_cache(/*config_model_catalog*/ None)
                .auth_manager()
                .is_none()
        );
    }
}

#[tokio::test]
async fn default_endpoint_tracks_cached_login_changes_without_resolving_its_credentials() {
    use codex_login::auth::ExternalAuth;
    use codex_login::auth::ExternalAuthFuture;
    use codex_login::auth::ExternalAuthRefreshContext;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct RoutingLogin {
        auth: CodexAuth,
        reads: Arc<AtomicUsize>,
    }

    impl ExternalAuth for RoutingLogin {
        fn resolve(&self) -> ExternalAuthFuture<'_, CodexAuth> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::ready(Ok(self.auth.clone())))
        }

        fn refresh(
            &self,
            _context: ExternalAuthRefreshContext,
        ) -> ExternalAuthFuture<'_, CodexAuth> {
            panic!("an independent provider key must not refresh the routing login")
        }
    }

    let login = CodexAuth::create_dummy_chatgpt_auth_for_testing();
    let credentials = AuthManager::from_auth_for_testing(login.clone());
    let provider = create_model_provider(
        ModelProviderInfo {
            experimental_bearer_token: Some("synthetic-provider-token".into()),
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        },
        Some(Arc::clone(&credentials)),
    );
    let reads = Arc::new(AtomicUsize::new(0));
    for (auth, expected_url) in [
        (
            CodexAuth::from_api_key("synthetic-unrelated-login-key"),
            "https://api.openai.com/v1",
        ),
        (login, codex_model_provider_info::CHATGPT_CODEX_BASE_URL),
    ] {
        credentials
            .set_external_auth(Arc::new(RoutingLogin {
                auth,
                reads: Arc::clone(&reads),
            }))
            .await
            .unwrap();
        let reads_before = reads.load(Ordering::SeqCst);
        assert!(provider.auth_manager().is_none());
        assert!(provider.auth().await.is_none());
        assert!(!provider.supports_attestation());
        assert_eq!(
            provider.api_provider().await.unwrap().base_url,
            expected_url
        );
        let (route, auth) = provider.api_provider_and_auth().await.unwrap();
        assert_eq!(route.base_url, expected_url);
        assert_eq!(
            auth.to_auth_headers().unwrap(),
            HeaderMap::from_iter([(
                AUTHORIZATION,
                HeaderValue::from_static("Bearer synthetic-provider-token"),
            )])
        );
        assert_eq!(reads.load(Ordering::SeqCst), reads_before);
    }
}

#[tokio::test]
async fn provider_env_key_never_falls_back_to_shared_openai_login() {
    const TEST_NAME: &str =
        "provider::auth_isolation_tests::provider_env_key_never_falls_back_to_shared_openai_login";
    const CHILD: &str = "CODEX_PROVIDER_AUTH_ISOLATION_TEST_CHILD";
    const KEY: &str = "CODEX_PROVIDER_AUTH_ISOLATION_SYNTHETIC_KEY";
    let Ok(mode) = std::env::var(CHILD) else {
        // Set/remove only a synthetic key in child processes; never mutate the
        // parent environment while other credential tests run concurrently.
        for mode in ["present", "empty", "missing"] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.arg("--exact").arg(TEST_NAME).env(CHILD, mode);
            if mode == "present" {
                command.env(KEY, "synthetic-env-provider-token");
            } else if mode == "empty" {
                command.env(KEY, "   ");
            } else {
                command.env_remove(KEY);
            }
            assert!(command.status().unwrap().success());
        }
        return;
    };
    for wire_api in [WireApi::Responses, WireApi::GrokResponses] {
        for requires_openai_auth in [false, true] {
            for fallback in [None, Some("synthetic-lower-priority-bearer")] {
                let provider = create_model_provider(
                    ModelProviderInfo {
                        name: "Independent provider".into(),
                        base_url: Some("https://provider.example.test/v1".into()),
                        env_key: Some(KEY.into()),
                        env_key_instructions: Some("Set the configured provider key".into()),
                        experimental_bearer_token: fallback.map(Into::into),
                        wire_api,
                        requires_openai_auth,
                        ..Default::default()
                    },
                    Some(AuthManager::from_auth_for_testing(
                        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
                    )),
                );
                assert!(provider.auth_manager().is_none());
                assert!(!provider.supports_attestation());
                // Account read describes the selected credential source, not whether
                // its key exists or will be accepted by a server. Missing keys must
                // not send the user through an unrelated OpenAI login flow.
                assert_eq!(
                    provider.account_state(),
                    Ok(ProviderAccountState {
                        account: None,
                        requires_openai_auth: false,
                    })
                );
                let auth = provider.api_auth().await;
                if mode == "present" {
                    assert_eq!(
                        auth.unwrap().to_auth_headers().unwrap(),
                        HeaderMap::from_iter([(
                            AUTHORIZATION,
                            HeaderValue::from_static("Bearer synthetic-env-provider-token"),
                        )])
                    );
                } else {
                    let error = auth.err().expect("a missing provider key must fail closed");
                    let codex_protocol::error::CodexErrorDetails::EnvVar(error) = error.details()
                    else {
                        panic!("expected a provider environment-variable error");
                    };
                    assert_eq!(error.var, KEY);
                    assert_eq!(
                        error.instructions.as_deref(),
                        Some("Set the configured provider key")
                    );
                }
            }
        }
    }
}

#[test]
fn command_auth_keeps_its_independent_provider_manager() {
    let credentials =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_chatgpt_auth_for_testing());
    let info = ModelProviderInfo {
        auth: Some(ModelProviderAuthInfo {
            command: "not-executed-by-this-ownership-test".into(),
            args: Vec::new(),
            timeout_ms: NonZeroU64::new(5_000).unwrap(),
            refresh_interval_ms: 300_000,
            cwd: std::env::current_dir().unwrap().try_into().unwrap(),
        }),
        requires_openai_auth: false,
        ..ModelProviderInfo::default()
    };
    assert_eq!(info.validate(), Ok(()));
    assert_eq!(
        ModelProviderInfo {
            requires_openai_auth: true,
            ..info.clone()
        }
        .validate(),
        Err("provider auth cannot be combined with requires_openai_auth".to_string())
    );
    let provider = create_model_provider(info, Some(Arc::clone(&credentials)));
    let selected = provider.auth_manager().unwrap();
    assert!(selected.has_external_auth());
    assert!(!Arc::ptr_eq(&selected, &credentials));
    assert!(!provider.supports_attestation());
    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: None,
            requires_openai_auth: false,
        })
    );
}
