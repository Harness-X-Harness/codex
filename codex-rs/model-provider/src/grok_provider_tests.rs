use std::path::PathBuf;
use std::sync::Arc;

use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::cache::ModelsCacheEntry;
use codex_models_manager::cache::ModelsCacheError;
use codex_models_manager::cache::ModelsCacheFuture;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

use crate::RemoteCompactionSupport;
use crate::create_model_provider;
use crate::grok_catalog::static_model_catalog;
use crate::grok_provider::is_grok_provider_info;
use crate::image_generation_policy;

fn provider_info(name: &str) -> ModelProviderInfo {
    ModelProviderInfo {
        name: name.to_string(),
        base_url: Some("https://example.test/v1".to_string()),
        ..ModelProviderInfo::default()
    }
}

fn replacement_catalog(slug: &str) -> ModelsResponse {
    let mut model = static_model_catalog()
        .models
        .into_iter()
        .next()
        .expect("bundled Grok catalog should contain a model");
    model.slug = slug.to_string();
    model.display_name = slug.to_string();
    ModelsResponse {
        models: vec![model],
    }
}

#[derive(Debug)]
struct PanicCache;

impl ModelsCache for PanicCache {
    fn load<'a>(
        &'a self,
        _client_version: &'a str,
    ) -> ModelsCacheFuture<'a, Result<Option<ModelsCacheEntry>, ModelsCacheError>> {
        panic!("Grok static catalog must not read the shared models cache")
    }

    fn store<'a>(
        &'a self,
        _entry: &'a ModelsCacheEntry,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        panic!("Grok static catalog must not write the shared models cache")
    }

    fn refresh_ttl<'a>(
        &'a self,
        _client_version: &'a str,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        panic!("Grok static catalog must not refresh the shared models cache")
    }
}

#[test]
fn grok_provider_identity_is_explicit_and_does_not_match_stock_profiles() {
    assert!(is_grok_provider_info(&provider_info("Grok")));
    assert!(is_grok_provider_info(&provider_info("gRoK")));
    assert!(!is_grok_provider_info(&provider_info("OpenAI")));
    assert!(!is_grok_provider_info(&provider_info("Custom")));
}

#[test]
fn grok_provider_identity_matches_serialized_wire_selector() {
    let mut info = provider_info("Custom");
    info.wire_api = WireApi::GrokResponses;
    assert!(is_grok_provider_info(&info));

    let provider = create_model_provider(info, /*auth_manager*/ None);
    assert!(provider.projects_tools_as_flat_functions());
}

#[tokio::test]
async fn grok_models_manager_uses_bundle_or_exact_config_replacement() {
    let provider = create_model_provider(provider_info("Grok"), /*auth_manager*/ None);
    let bundled_catalog = static_model_catalog();

    assert_eq!(
        provider
            .models_manager(PathBuf::new(), /*config_model_catalog*/ None)
            .get_remote_models()
            .await,
        bundled_catalog.models
    );

    let configured_catalog = replacement_catalog("configured-grok");
    assert_eq!(
        provider
            .models_manager(PathBuf::new(), Some(configured_catalog.clone()))
            .get_remote_models()
            .await,
        configured_catalog.models
    );
}

#[tokio::test]
async fn grok_models_manager_never_consults_remote_cache() {
    let provider = create_model_provider(provider_info("Grok"), /*auth_manager*/ None);
    let bundled_catalog = static_model_catalog();

    assert_eq!(
        provider
            .models_manager_with_cache(/*config_model_catalog*/ None, Arc::new(PanicCache),)
            .get_remote_models()
            .await,
        bundled_catalog.models
    );
}

#[tokio::test]
async fn non_grok_provider_keeps_stock_config_catalog_behavior() {
    let provider = create_model_provider(provider_info("Custom"), /*auth_manager*/ None);
    let configured_catalog = replacement_catalog("stock-custom-model");

    assert_eq!(
        provider
            .models_manager(PathBuf::new(), Some(configured_catalog.clone()))
            .get_remote_models()
            .await,
        configured_catalog.models
    );
    assert!(!provider.projects_tools_as_flat_functions());
    assert_eq!(image_generation_policy(&provider), None);
}

#[test]
fn grok_tool_and_image_contract_is_explicit() {
    let provider = create_model_provider(provider_info("Grok"), /*auth_manager*/ None);
    let capabilities = provider.capabilities();

    assert!(capabilities.namespace_tools);
    assert!(capabilities.web_search);
    assert!(capabilities.external_web_access);
    assert!(capabilities.image_generation);
    assert!(provider.projects_tools_as_flat_functions());
    assert_eq!(
        image_generation_policy(&provider).map(|policy| policy.max_edit_images),
        Some(3)
    );
}

#[test]
fn stock_openai_image_policy_keeps_five_edit_images() {
    let provider = create_model_provider(provider_info("OpenAI"), /*auth_manager*/ None);

    assert_eq!(
        image_generation_policy(&provider).map(|policy| policy.max_edit_images),
        Some(5)
    );
}

#[test]
fn grok_does_not_advertise_remote_compaction_v2() {
    let provider = create_model_provider(provider_info("Grok"), /*auth_manager*/ None);

    assert_eq!(
        provider.capabilities().remote_compaction,
        RemoteCompactionSupport::Unsupported
    );
}

#[test]
fn grok_internal_tasks_use_bundled_model_not_openai_ids() {
    let grok = create_model_provider(provider_info("Grok"), /*auth_manager*/ None);
    let openai = create_model_provider(provider_info("OpenAI"), /*auth_manager*/ None);
    let custom = create_model_provider(provider_info("Custom"), /*auth_manager*/ None);
    let bundled = static_model_catalog()
        .models
        .into_iter()
        .next()
        .expect("bundled Grok catalog should contain a model");

    assert_eq!(
        (
            grok.approval_review_preferred_model(),
            grok.memory_extraction_preferred_model(),
            grok.memory_consolidation_preferred_model(),
        ),
        (
            bundled.slug.as_str(),
            bundled.slug.as_str(),
            bundled.slug.as_str(),
        )
    );
    assert_ne!(
        grok.approval_review_preferred_model(),
        openai.approval_review_preferred_model()
    );
    assert_ne!(
        grok.memory_extraction_preferred_model(),
        openai.memory_extraction_preferred_model()
    );
    assert_ne!(
        grok.memory_consolidation_preferred_model(),
        openai.memory_consolidation_preferred_model()
    );
    assert_eq!(
        (
            custom.approval_review_preferred_model(),
            custom.memory_extraction_preferred_model(),
            custom.memory_consolidation_preferred_model(),
        ),
        (
            openai.approval_review_preferred_model(),
            openai.memory_extraction_preferred_model(),
            openai.memory_consolidation_preferred_model(),
        )
    );
}

#[test]
fn grok_ultra_resolves_to_xhigh_only_at_request_normalization() {
    let model = static_model_catalog()
        .models
        .into_iter()
        .next()
        .expect("bundled Grok catalog should contain a model");

    assert!(
        model
            .supported_reasoning_levels
            .iter()
            .any(|preset| preset.effort == ReasoningEffort::Ultra),
        "logical Ultra remains selectable in Codex model state"
    );
    assert_eq!(
        model.resolve_reasoning_effort(ReasoningEffort::Ultra),
        ReasoningEffort::XHigh,
        "stock 0.154 request normalization projects logical Ultra to Grok xhigh"
    );
}
