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

use super::ModelAvailability;
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
async fn static_catalog_declares_whether_it_constrains_model_availability() -> CoreResult<()> {
    let catalog = ModelsResponse {
        models: vec![model_info_from_slug("catalog-model")],
    };
    let authoritative = StaticModelsManager::new(/*auth_manager*/ None, catalog.clone());
    let metadata_only =
        StaticModelsManager::new_unconstrained(/*auth_manager*/ None, catalog.clone());

    assert_eq!(
        authoritative
            .model_availability(
                "missing-model",
                RefreshStrategy::Offline,
                HTTP_CLIENT_FACTORY,
            )
            .await?,
        ModelAvailability::Unavailable
    );
    assert_eq!(
        metadata_only
            .model_availability(
                "missing-model",
                RefreshStrategy::Offline,
                HTTP_CLIENT_FACTORY,
            )
            .await?,
        ModelAvailability::Unconstrained
    );
    Ok(())
}
