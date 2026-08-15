use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_model_provider_info::WireApi;
use codex_models_manager::cache::ModelsCatalogIdentity;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ModelProviderRegistry;
use super::ProviderRegistration;
use super::ResolvedProviderSelection;
use super::provider_models_home;
use crate::ModelProvider;
use crate::ModelProviderFuture;
use crate::ProviderAccountResult;
use crate::ProviderAccountState;
use crate::ProviderAuthScope;
use crate::ProviderRequestSetup;
use crate::SharedModelProvider;
use crate::create_model_provider;

#[test]
fn provider_cache_roots_are_stable_and_isolated() {
    let root = Path::new("/tmp/codex-home");
    assert_eq!(provider_models_home(root, "openai"), root);
    assert_eq!(
        provider_models_home(root, "grok"),
        root.join("model-providers").join("67726f6b")
    );
    assert_ne!(
        provider_models_home(root, "namespace/a"),
        provider_models_home(root, "namespace:a")
    );
}

#[tokio::test]
async fn registered_catalogs_are_labeled_and_duplicate_slugs_fail_atomically() {
    let registry = test_registry(&["openai-model"], &["grok-model"]);
    let models = registry
        .list_models(RefreshStrategy::Offline, test_http_client_factory())
        .await
        .expect("distinct catalogs should merge");
    assert_eq!(
        models
            .iter()
            .map(|model| (model.model.as_str(), model.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("openai-model", "ChatGPT · openai-model"),
            ("grok-model", "Grok · grok-model"),
        ]
    );

    let duplicate = test_registry(&["shared-model"], &["shared-model"])
        .list_models(RefreshStrategy::Offline, test_http_client_factory())
        .await
        .expect_err("duplicate slugs must reject the complete catalog");
    assert!(duplicate.to_string().contains("advertised by both"));
}

#[tokio::test]
async fn explicit_selections_resolve_and_bound_models_cannot_cross_providers() {
    let registry = test_registry(&["openai-model"], &["grok-model"]);
    let factory = test_http_client_factory();

    let inferred = registry
        .resolve_new_thread_selection(
            Some("grok-model"),
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect("known model should resolve");
    assert_eq!(
        inferred,
        Some(ResolvedProviderSelection {
            model: "grok-model".to_string(),
            provider_id: "grok".to_string(),
        })
    );

    let provider_default = registry
        .resolve_new_thread_selection(
            /*requested_model*/ None,
            Some("grok"),
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect("known provider should select its default");
    assert_eq!(provider_default, inferred);

    let mismatch = registry
        .resolve_new_thread_selection(
            Some("grok-model"),
            Some("openai"),
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect_err("model and provider must have the same catalog owner");
    assert!(mismatch.to_string().contains("belongs to provider `grok`"));

    let cross_provider = registry
        .validate_bound_model("openai", "grok-model", RefreshStrategy::Offline, factory)
        .await
        .expect_err("bound provider must reject another provider's model");
    assert!(cross_provider.to_string().contains("start a new thread"));
}

#[tokio::test]
async fn existing_thread_selection_preserves_provider_and_available_model() {
    let registry = test_registry(&["openai-model", "openai-model-2"], &["grok-model"]);
    let factory = test_http_client_factory();

    let persisted = registry
        .resolve_existing_thread_selection(
            "grok",
            Some("grok-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect("available persisted model should retain its Provider binding");
    assert_eq!(
        persisted,
        Some(ResolvedProviderSelection {
            model: "grok-model".to_string(),
            provider_id: "grok".to_string(),
        })
    );

    let same_provider = registry
        .resolve_existing_thread_selection(
            "openai",
            Some("openai-model"),
            Some("openai-model-2"),
            Some("openai"),
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect("same-provider model change should resolve");
    assert_eq!(
        same_provider,
        Some(ResolvedProviderSelection {
            model: "openai-model-2".to_string(),
            provider_id: "openai".to_string(),
        })
    );

    let provider_mismatch = registry
        .resolve_existing_thread_selection(
            "grok",
            Some("grok-model"),
            Some("openai-model"),
            Some("openai"),
            RefreshStrategy::Offline,
            factory,
        )
        .await
        .expect_err("existing thread provider must be immutable");
    assert!(provider_mismatch.to_string().contains("start a new thread"));
}

#[tokio::test]
async fn unavailable_provider_does_not_erase_healthy_catalogs() {
    let registry = ModelProviderRegistry::new(
        [
            test_registration(
                OPENAI_PROVIDER_ID,
                "OpenAI",
                WireApi::Responses,
                &["openai-model"],
            ),
            unavailable_registration("grok", "Grok", WireApi::GrokResponses),
        ],
        OPENAI_PROVIDER_ID,
    )
    .expect("test registrations should construct the registry");
    let factory = test_http_client_factory();

    let models = registry
        .list_models(RefreshStrategy::Online, factory.clone())
        .await
        .expect("healthy provider choices must survive another Authority outage");
    assert_eq!(
        models
            .into_iter()
            .map(|model| model.model)
            .collect::<Vec<_>>(),
        vec!["openai-model"]
    );

    let unavailable = registry
        .validate_bound_model("grok", "grok-model", RefreshStrategy::Online, factory)
        .await
        .expect_err("bound unavailable provider must retain its Authority error");
    assert!(unavailable.to_string().contains("AuthorityUnavailable"));

    let healthy_absence = registry
        .resolve_new_thread_selection(
            Some("missing-openai-model"),
            Some(OPENAI_PROVIDER_ID),
            RefreshStrategy::Online,
            test_http_client_factory(),
        )
        .await
        .expect_err("explicit healthy Provider can prove model absence");
    assert!(healthy_absence.to_string().contains("ModelUnavailable"));

    let unknown_owner = registry
        .resolve_new_thread_selection(
            Some("unknown-owner-model"),
            None,
            RefreshStrategy::Online,
            test_http_client_factory(),
        )
        .await
        .expect_err("unavailable Provider prevents proof of global model absence");
    assert!(unknown_owner.to_string().contains("AuthorityUnavailable"));
}

#[tokio::test]
async fn empty_healthy_catalog_does_not_hide_another_authority_outage() {
    let healthy_empty = ModelProviderRegistry::new(
        [test_registration(
            OPENAI_PROVIDER_ID,
            "OpenAI",
            WireApi::Responses,
            &[],
        )],
        OPENAI_PROVIDER_ID,
    )
    .expect("healthy empty registration should construct the registry");
    assert!(
        healthy_empty
            .list_models(RefreshStrategy::Offline, test_http_client_factory())
            .await
            .expect("a successful empty catalog is a healthy result")
            .is_empty()
    );

    let registry = ModelProviderRegistry::new(
        [
            test_registration(OPENAI_PROVIDER_ID, "OpenAI", WireApi::Responses, &[]),
            unavailable_registration("grok", "Grok", WireApi::GrokResponses),
        ],
        OPENAI_PROVIDER_ID,
    )
    .expect("test registrations should construct the registry");

    let error = registry
        .list_models(RefreshStrategy::Online, test_http_client_factory())
        .await
        .expect_err("an empty healthy catalog cannot erase another Authority outage");

    assert!(error.to_string().contains("AuthorityUnavailable"));
}

#[tokio::test]
async fn successful_catalog_absence_is_model_unavailable() {
    let registry = test_registry(&["openai-model"], &[]);
    let factory = test_http_client_factory();

    let unavailable = registry
        .validate_bound_model(
            "grok",
            "removed-grok-model",
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect_err("successful absence must be distinct from Authority failure");
    assert!(unavailable.to_string().contains("ModelUnavailable"));

    let persisted = registry
        .resolve_existing_thread_selection(
            "grok",
            Some("removed-grok-model"),
            None,
            None,
            RefreshStrategy::Offline,
            factory,
        )
        .await
        .expect_err("persisted model must be validated against the successful catalog");
    assert!(persisted.to_string().contains("ModelUnavailable"));

    let reassigned = test_registry(&["reassigned-model"], &[]);
    let reassigned_error = reassigned
        .resolve_existing_thread_selection(
            "grok",
            Some("reassigned-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            test_http_client_factory(),
        )
        .await
        .expect_err("another Provider cannot replace the bound Authority's absence");
    assert!(reassigned_error.to_string().contains("ModelUnavailable"));
    assert!(!reassigned_error.to_string().contains("belongs to provider"));
}

#[tokio::test]
async fn single_authoritative_provider_does_not_bypass_availability() {
    let unavailable = ModelProviderRegistry::new(
        [unavailable_registration(
            "grok",
            "Grok",
            WireApi::GrokResponses,
        )],
        "grok",
    )
    .expect("single authoritative registration should construct the registry");
    let unavailable_error = unavailable
        .validate_bound_model(
            "grok",
            "grok-model",
            RefreshStrategy::Online,
            test_http_client_factory(),
        )
        .await
        .expect_err("single authoritative Provider must preserve an Authority outage");
    assert!(
        unavailable_error
            .to_string()
            .contains("AuthorityUnavailable")
    );

    let empty = authoritative_registration("grok", "Grok", WireApi::GrokResponses, Vec::new());
    let removed = ModelProviderRegistry::new([empty], "grok")
        .expect("single authoritative registration should construct the registry");
    let removed_error = removed
        .resolve_existing_thread_selection(
            "grok",
            Some("grok-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Online,
            test_http_client_factory(),
        )
        .await
        .expect_err("successful authoritative absence must reject a persisted model");
    assert!(removed_error.to_string().contains("ModelUnavailable"));
}

#[tokio::test]
async fn non_authoritative_provider_refresh_failure_preserves_stock_catalog() {
    let info = ModelProviderInfo {
        name: "OpenAI".to_string(),
        wire_api: WireApi::Responses,
        ..ModelProviderInfo::default()
    };
    let manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new_without_cache(
        Arc::new(TestModelsEndpoint {
            authoritative: false,
            models: None,
        }),
        /*auth_manager*/ None,
    ));
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter { info, manager });
    let registry = ModelProviderRegistry::new(
        [ProviderRegistration::new(
            OPENAI_PROVIDER_ID,
            "ChatGPT",
            provider,
            PathBuf::from("/tmp/test-provider-models"),
            /*config_model_catalog*/ None,
        )],
        OPENAI_PROVIDER_ID,
    )
    .expect("stock registration should construct the registry");

    let models = registry
        .list_models(RefreshStrategy::Online, test_http_client_factory())
        .await
        .expect("non-authoritative refresh failure must retain the stock catalog");

    assert!(!models.is_empty());
}

#[tokio::test]
async fn one_registration_preserves_stock_selection_semantics() -> codex_protocol::error::Result<()>
{
    let registry = ModelProviderRegistry::new(
        [unconstrained_test_registration(
            OPENAI_PROVIDER_ID,
            "OpenAI",
            WireApi::Responses,
            &["openai-model"],
        )],
        OPENAI_PROVIDER_ID,
    )
    .expect("single registration should construct the registry");
    let factory = test_http_client_factory();

    assert!(!registry.requires_bound_history());
    assert_eq!(
        registry.default_thread_provider_filter(),
        Some(vec![OPENAI_PROVIDER_ID.to_string()])
    );
    assert_eq!(
        registry
            .resolve_new_thread_selection(
                Some("unlisted-model"),
                /*requested_provider_id*/ None,
                RefreshStrategy::Offline,
                factory.clone(),
            )
            .await?,
        None
    );
    assert_eq!(
        registry
            .resolve_existing_thread_selection(
                OPENAI_PROVIDER_ID,
                Some("openai-model"),
                Some("unlisted-model"),
                Some(OPENAI_PROVIDER_ID),
                RefreshStrategy::Offline,
                factory.clone(),
            )
            .await?,
        None
    );
    registry
        .validate_bound_model(
            OPENAI_PROVIDER_ID,
            "unlisted-model",
            RefreshStrategy::Offline,
            factory,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn registration_add_remove_and_restore_use_only_the_public_seam() {
    let openai = unconstrained_test_registration(
        OPENAI_PROVIDER_ID,
        "OpenAI",
        WireApi::Responses,
        &["openai-model"],
    );
    let test_provider = test_registration(
        "test-provider",
        "Test Provider",
        WireApi::Responses,
        &["test-model"],
    );
    let registered =
        ModelProviderRegistry::new([openai.clone(), test_provider.clone()], OPENAI_PROVIDER_ID)
            .expect("explicit registrations should construct the registry");
    let runtime = registered
        .resolve_runtime("test-provider")
        .expect("registration should resolve its complete runtime");
    assert_eq!(runtime.provider_id(), "test-provider");
    assert_eq!(runtime.provider().info().name, "Test Provider");
    assert_eq!(
        runtime
            .models_manager()
            .list_models(RefreshStrategy::Offline, test_http_client_factory())
            .await
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>(),
        vec!["test-model"]
    );
    let selection = registered
        .resolve_new_thread_selection(
            Some("test-model"),
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            test_http_client_factory(),
        )
        .await
        .expect("registered test provider should resolve");
    assert_eq!(
        selection,
        Some(ResolvedProviderSelection {
            model: "test-model".to_string(),
            provider_id: "test-provider".to_string(),
        })
    );

    let removed = ModelProviderRegistry::new([openai], OPENAI_PROVIDER_ID)
        .expect("remaining registration should construct the registry");
    assert_eq!(
        removed
            .list_models(RefreshStrategy::Offline, test_http_client_factory())
            .await
            .expect("remaining registration should keep its catalog")
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>(),
        vec!["openai-model"]
    );
    let removed_new_selection = removed
        .resolve_new_thread_selection(
            Some("test-model"),
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            test_http_client_factory(),
        )
        .await
        .expect("single-provider compatibility keeps arbitrary model overrides");
    assert_eq!(removed_new_selection, None);
    let unavailable = removed
        .resolve_existing_thread_selection(
            "test-provider",
            Some("test-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            test_http_client_factory(),
        )
        .await
        .expect_err("removed provider must not reroute its bound thread");
    assert!(unavailable.to_string().contains("ProviderUnavailable"));

    let restored = ModelProviderRegistry::new([test_provider], "test-provider")
        .expect("stable registration should be restorable");
    let restored_selection = restored
        .resolve_existing_thread_selection(
            "test-provider",
            Some("test-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            test_http_client_factory(),
        )
        .await
        .expect("restored provider should resolve its bound thread");
    assert_eq!(restored_selection, None);
    let restored_runtime = restored
        .resolve_runtime("test-provider")
        .expect("restored provider runtime should resolve by its stable ID");
    assert_eq!(restored_runtime.provider().info().name, "Test Provider");
}

#[test]
fn provider_owned_credentials_do_not_inherit_chatgpt_auth() {
    let mut info = ModelProviderInfo {
        name: "Grok".to_string(),
        env_key: Some("GROK_API_KEY".to_string()),
        wire_api: WireApi::GrokResponses,
        ..ModelProviderInfo::default()
    };
    info.requires_openai_auth = false;
    let chatgpt_auth =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_chatgpt_auth_for_testing());
    let provider = create_model_provider(info, Some(chatgpt_auth));

    assert!(provider.auth_manager().is_none());
}

fn test_registry(openai_slugs: &[&str], grok_slugs: &[&str]) -> ModelProviderRegistry {
    ModelProviderRegistry::new(
        [
            test_registration(
                OPENAI_PROVIDER_ID,
                "OpenAI",
                WireApi::Responses,
                openai_slugs,
            ),
            test_registration("grok", "Grok", WireApi::GrokResponses, grok_slugs),
        ],
        OPENAI_PROVIDER_ID,
    )
    .expect("test registrations should construct the registry")
}

fn test_registration(
    id: &str,
    display_name: &str,
    wire_api: WireApi,
    slugs: &[&str],
) -> ProviderRegistration {
    let info = ModelProviderInfo {
        name: display_name.to_string(),
        wire_api,
        ..ModelProviderInfo::default()
    };
    let manager: SharedModelsManager = Arc::new(StaticModelsManager::new(
        /*auth_manager*/ None,
        ModelsResponse {
            models: slugs.iter().map(|slug| test_model(slug)).collect(),
        },
    ));
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter { info, manager });
    let picker_label = if id == OPENAI_PROVIDER_ID {
        "ChatGPT"
    } else {
        display_name
    };
    ProviderRegistration::new(
        id,
        picker_label,
        provider,
        PathBuf::from("/tmp/test-provider-models"),
        /*config_model_catalog*/ None,
    )
}

fn unconstrained_test_registration(
    id: &str,
    display_name: &str,
    wire_api: WireApi,
    slugs: &[&str],
) -> ProviderRegistration {
    let info = ModelProviderInfo {
        name: display_name.to_string(),
        wire_api,
        ..ModelProviderInfo::default()
    };
    let manager: SharedModelsManager = Arc::new(StaticModelsManager::new_unconstrained(
        /*auth_manager*/ None,
        ModelsResponse {
            models: slugs.iter().map(|slug| test_model(slug)).collect(),
        },
    ));
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter { info, manager });
    let picker_label = if id == OPENAI_PROVIDER_ID {
        "ChatGPT"
    } else {
        display_name
    };
    ProviderRegistration::new(
        id,
        picker_label,
        provider,
        PathBuf::from("/tmp/test-provider-models"),
        /*config_model_catalog*/ None,
    )
}

fn unavailable_registration(
    id: &str,
    display_name: &str,
    wire_api: WireApi,
) -> ProviderRegistration {
    let info = ModelProviderInfo {
        name: display_name.to_string(),
        wire_api,
        ..ModelProviderInfo::default()
    };
    let manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new_without_cache(
        Arc::new(TestModelsEndpoint {
            authoritative: true,
            models: None,
        }),
        /*auth_manager*/ None,
    ));
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter { info, manager });
    ProviderRegistration::new(
        id,
        display_name,
        provider,
        PathBuf::from("/tmp/test-provider-models"),
        /*config_model_catalog*/ None,
    )
}

fn authoritative_registration(
    id: &str,
    display_name: &str,
    wire_api: WireApi,
    models: Vec<ModelInfo>,
) -> ProviderRegistration {
    let info = ModelProviderInfo {
        name: display_name.to_string(),
        wire_api,
        ..ModelProviderInfo::default()
    };
    let manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new_without_cache(
        Arc::new(TestModelsEndpoint {
            authoritative: true,
            models: Some(models),
        }),
        /*auth_manager*/ None,
    ));
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter { info, manager });
    ProviderRegistration::new(
        id,
        display_name,
        provider,
        PathBuf::from("/tmp/test-provider-models"),
        /*config_model_catalog*/ None,
    )
}

#[derive(Debug)]
struct TestCatalogProviderAdapter {
    info: ModelProviderInfo,
    manager: SharedModelsManager,
}

impl ModelProvider for TestCatalogProviderAdapter {
    fn info(&self) -> &ModelProviderInfo {
        &self.info
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        None
    }

    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        Box::pin(async { None })
    }

    fn account_state(&self) -> ProviderAccountResult {
        Ok(ProviderAccountState {
            account: None,
            requires_openai_auth: false,
        })
    }

    fn request_setup(
        &self,
        scope: ProviderAuthScope,
    ) -> ModelProviderFuture<'_, CoreResult<ProviderRequestSetup>> {
        let delegate = create_model_provider(self.info.clone(), /*auth_manager*/ None);
        Box::pin(async move { delegate.request_setup(scope).await })
    }

    fn models_manager(
        &self,
        _codex_home: PathBuf,
        _config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::clone(&self.manager)
    }
}

#[derive(Debug)]
struct TestModelsEndpoint {
    authoritative: bool,
    models: Option<Vec<ModelInfo>>,
}

impl ModelsEndpointClient for TestModelsEndpoint {
    fn catalog_identity(&self) -> ModelsCatalogIdentity {
        ModelsCatalogIdentity::new("test-unavailable-authority", "test-decoder-v1")
    }

    fn has_command_auth(&self) -> bool {
        true
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(async { false })
    }

    fn remote_catalog_is_authoritative(&self) -> bool {
        self.authoritative
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(async move {
            match self.models.as_ref() {
                Some(models) => Ok((models.clone(), None)),
                None => Err(CodexErr::InvalidRequest(
                    "test Authority is unavailable".to_string(),
                )),
            }
        })
    }
}

fn test_model(slug: &str) -> ModelInfo {
    serde_json::from_value(json!({
        "slug": slug,
        "display_name": slug,
        "description": null,
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "upgrade": null,
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "truncation_policy": {"mode": "bytes", "limit": 10_000},
        "supports_parallel_tool_calls": false,
        "supports_image_detail_original": false,
        "context_window": 272_000,
        "max_context_window": 272_000,
        "experimental_supported_tools": [],
    }))
    .expect("valid test model")
}

fn test_http_client_factory() -> HttpClientFactory {
    HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
}
