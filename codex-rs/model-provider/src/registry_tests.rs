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
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
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
async fn existing_thread_selection_preserves_provider_and_persisted_model() {
    let registry = test_registry(&["openai-model", "openai-model-2"], &["grok-model"]);
    let factory = test_http_client_factory();

    let persisted = registry
        .resolve_existing_thread_selection(
            "grok",
            Some("retired-grok-model"),
            /*requested_model*/ None,
            /*requested_provider_id*/ None,
            RefreshStrategy::Offline,
            factory.clone(),
        )
        .await
        .expect("persisted model should not require current catalog membership");
    assert_eq!(
        persisted,
        Some(ResolvedProviderSelection {
            model: "retired-grok-model".to_string(),
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
    let provider: SharedModelProvider = Arc::new(TestCatalogProviderAdapter {
        info,
        catalog: ModelsResponse {
            models: slugs.iter().map(|slug| test_model(slug)).collect(),
        },
    });
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

#[derive(Debug)]
struct TestCatalogProviderAdapter {
    info: ModelProviderInfo,
    catalog: ModelsResponse,
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

    fn models_manager(
        &self,
        _codex_home: PathBuf,
        _config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::new(StaticModelsManager::new(
            /*auth_manager*/ None,
            self.catalog.clone(),
        ))
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
