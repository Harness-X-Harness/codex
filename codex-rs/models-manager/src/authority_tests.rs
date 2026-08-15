use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use chrono::Utc;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;

use super::ModelResolution;
use super::ModelSelection;
use super::ModelsEndpointClient;
use super::ModelsEndpointFuture;
use super::ModelsManager;
use super::OpenAiModelsManager;
use super::RefreshStrategy;
use super::StaticModelsManager;
use crate::cache::ModelsCache;
use crate::cache::ModelsCacheEntry;
use crate::cache::ModelsCacheError;
use crate::cache::ModelsCacheFuture;
use crate::cache::ModelsCatalogIdentity;
use crate::config::ModelsManagerConfig;
use crate::model_info::model_info_from_slug;

const AUTHORITY: &str = "test-authority";
const DECODER_VERSION: &str = "test-decoder-v2";
const OLD_DECODER_VERSION: &str = "test-decoder-v1";
const HTTP_CLIENT_FACTORY: HttpClientFactory =
    HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);

#[derive(Debug)]
struct TestCache {
    entry: Mutex<Option<ModelsCacheEntry>>,
}

impl TestCache {
    fn new(entry: ModelsCacheEntry) -> Arc<Self> {
        Arc::new(Self {
            entry: Mutex::new(Some(entry)),
        })
    }
}

impl ModelsCache for TestCache {
    fn load<'a>(
        &'a self,
        _client_version: &'a str,
        _catalog_identity: &'a ModelsCatalogIdentity,
    ) -> ModelsCacheFuture<'a, Result<Option<ModelsCacheEntry>, ModelsCacheError>> {
        Box::pin(async move {
            Ok(self
                .entry
                .lock()
                .expect("cache lock should not be poisoned")
                .clone())
        })
    }

    fn store<'a>(
        &'a self,
        entry: &'a ModelsCacheEntry,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(async move {
            *self
                .entry
                .lock()
                .expect("cache lock should not be poisoned") = Some(entry.clone());
            Ok(())
        })
    }

    fn refresh_ttl<'a>(
        &'a self,
        _client_version: &'a str,
        _catalog_identity: &'a ModelsCatalogIdentity,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug)]
struct TestEndpoint {
    models: Option<Vec<ModelInfo>>,
    fetch_count: AtomicUsize,
}

impl TestEndpoint {
    fn available(models: Vec<ModelInfo>) -> Arc<Self> {
        Arc::new(Self {
            models: Some(models),
            fetch_count: AtomicUsize::new(0),
        })
    }

    fn unavailable() -> Arc<Self> {
        Arc::new(Self {
            models: None,
            fetch_count: AtomicUsize::new(0),
        })
    }
}

impl ModelsEndpointClient for TestEndpoint {
    fn catalog_identity(&self) -> ModelsCatalogIdentity {
        ModelsCatalogIdentity::new(AUTHORITY, DECODER_VERSION)
    }

    fn has_command_auth(&self) -> bool {
        false
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(async { false })
    }

    fn remote_catalog_is_authoritative(&self) -> bool {
        true
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(async move {
            self.fetch_count.fetch_add(1, Ordering::SeqCst);
            self.models
                .clone()
                .map(|models| (models, None))
                .ok_or_else(|| CodexErr::InvalidRequest("test authority failure".to_string()))
        })
    }
}

fn cache_entry(catalog_identity: Option<ModelsCatalogIdentity>) -> ModelsCacheEntry {
    ModelsCacheEntry {
        fetched_at: Utc::now(),
        etag: None,
        client_version: Some(crate::client_version_to_whole()),
        catalog_identity,
        models: vec![model_info_from_slug("cached-model")],
    }
}

#[tokio::test]
async fn legacy_or_old_decoder_cache_cannot_satisfy_authority() {
    for catalog_identity in [
        None,
        Some(ModelsCatalogIdentity::new(AUTHORITY, OLD_DECODER_VERSION)),
    ] {
        let cache = TestCache::new(cache_entry(catalog_identity));
        let endpoint = TestEndpoint::available(vec![model_info_from_slug("live-model")]);
        let manager = OpenAiModelsManager::new_with_cache(cache, endpoint.clone(), None);

        let catalog = manager
            .load_model_catalog(RefreshStrategy::OnlineIfUncached, HTTP_CLIENT_FACTORY)
            .await
            .expect("ineligible cache should cause an authoritative fetch");

        assert_eq!(
            catalog
                .models
                .into_iter()
                .map(|model| model.slug)
                .collect::<Vec<_>>(),
            vec!["live-model"]
        );
        assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn eligible_cache_preserves_availability_after_refresh_failure() {
    let cache = TestCache::new(cache_entry(Some(ModelsCatalogIdentity::new(
        AUTHORITY,
        DECODER_VERSION,
    ))));
    let endpoint = TestEndpoint::unavailable();
    let manager = OpenAiModelsManager::new_with_cache(cache, endpoint.clone(), None);

    let catalog = manager
        .load_model_catalog(RefreshStrategy::Online, HTTP_CLIENT_FACTORY)
        .await
        .expect("fresh cache of the same Authority may survive refresh failure");

    assert_eq!(catalog.models[0].slug, "cached-model");
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unavailable_authority_without_eligible_cache_remains_an_error() {
    let endpoint = TestEndpoint::unavailable();
    let manager = OpenAiModelsManager::new_without_cache(endpoint.clone(), None);

    manager
        .load_model_catalog(RefreshStrategy::Online, HTTP_CLIENT_FACTORY)
        .await
        .expect_err("an unavailable Authority must not become an empty catalog");
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn successful_empty_authoritative_catalog_is_not_unavailable() {
    let endpoint = TestEndpoint::available(Vec::new());
    let manager = OpenAiModelsManager::new_without_cache(endpoint.clone(), None);

    let catalog = manager
        .load_model_catalog(RefreshStrategy::Online, HTTP_CLIENT_FACTORY)
        .await
        .expect("successful empty catalog is an observed Authority result");

    assert!(catalog.models.is_empty());
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn authoritative_resolution_returns_model_and_picker_from_one_fetch() -> CoreResult<()> {
    let endpoint = TestEndpoint::available(vec![model_info_from_slug("catalog-model")]);
    let manager = OpenAiModelsManager::new_without_cache(endpoint.clone(), None);

    let ModelResolution::Resolved {
        model_info,
        available_models,
    } = manager
        .resolve_model_profile(
            ModelSelection::Exact("catalog-model"),
            &ModelsManagerConfig::default(),
            RefreshStrategy::Online,
            HTTP_CLIENT_FACTORY,
        )
        .await?
    else {
        panic!("authoritative catalog contains the requested model");
    };

    assert_eq!(model_info.slug, "catalog-model");
    assert_eq!(available_models.len(), 1);
    assert_eq!(available_models[0].model, "catalog-model");
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn authoritative_default_selection_and_profile_use_one_fetch() -> CoreResult<()> {
    let mut default_model = model_info_from_slug("catalog-default");
    default_model.priority = 0;
    let mut other_model = model_info_from_slug("catalog-other");
    other_model.priority = 1;
    let endpoint = TestEndpoint::available(vec![other_model, default_model]);
    let manager = OpenAiModelsManager::new_without_cache(endpoint.clone(), None);

    let ModelResolution::Resolved {
        model_info,
        available_models,
    } = manager
        .resolve_model_profile(
            ModelSelection::ProviderDefault,
            &ModelsManagerConfig::default(),
            RefreshStrategy::Online,
            HTTP_CLIENT_FACTORY,
        )
        .await?
    else {
        panic!("authoritative catalog should provide a default model");
    };

    assert_eq!(model_info.slug, "catalog-default");
    assert_eq!(available_models.len(), 2);
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn authoritative_preferred_model_falls_back_within_one_snapshot() -> CoreResult<()> {
    let mut default_model = model_info_from_slug("catalog-default");
    default_model.priority = 0;
    let endpoint = TestEndpoint::available(vec![default_model]);
    let manager = OpenAiModelsManager::new_without_cache(endpoint.clone(), None);

    let ModelResolution::Resolved {
        model_info,
        available_models,
    } = manager
        .resolve_model_profile(
            ModelSelection::PreferRequested("retired-model"),
            &ModelsManagerConfig::default(),
            RefreshStrategy::Online,
            HTTP_CLIENT_FACTORY,
        )
        .await?
    else {
        panic!("provider fallback should select the observed default model");
    };

    assert_eq!(model_info.slug, "catalog-default");
    assert_eq!(available_models.len(), 1);
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn static_catalog_resolves_or_rejects_from_one_snapshot() -> CoreResult<()> {
    let catalog = ModelsResponse {
        models: vec![model_info_from_slug("catalog-model")],
    };
    let authoritative = StaticModelsManager::new(/*auth_manager*/ None, catalog.clone());
    let metadata_only =
        StaticModelsManager::new_unconstrained(/*auth_manager*/ None, catalog.clone());

    let config = ModelsManagerConfig::default();
    assert!(matches!(
        authoritative
            .resolve_model_profile(ModelSelection::Exact("missing-model"), &config),
        ModelResolution::Unavailable { model } if model == "missing-model"
    ));
    let ModelResolution::Resolved {
        model_info,
        available_models,
    } = metadata_only
        .resolve_model_profile(ModelSelection::Exact("missing-model"), &config)
    else {
        panic!("metadata-only catalog must preserve unconstrained model identifiers");
    };
    assert_eq!(model_info.slug, "missing-model");
    assert!(model_info.used_fallback_model_metadata);
    assert_eq!(available_models.len(), 1);
    Ok(())
}
