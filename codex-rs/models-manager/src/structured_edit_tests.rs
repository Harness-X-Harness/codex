use super::*;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::StructuredEditToolType;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn structured_edit_capability_survives_file_cache_and_model_loading() {
    let home = tempdir().unwrap();
    let cache = Arc::new(FileModelsCache::new(
        home.path().join(MODEL_CACHE_FILE),
        DEFAULT_MODEL_CACHE_TTL,
    ));
    let mut enabled = remote_model("provider-neutral-editor", "Editor", /*priority*/ 0);
    enabled.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
    enabled.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
    let stock = remote_model("provider-neutral-stock", "Stock", /*priority*/ 1);
    let entry = ModelsCacheEntry {
        fetched_at: Utc::now(),
        etag: Some("editor-capability".to_string()),
        client_version: Some(crate::client_version_to_whole()),
        identity: Some("test-provider".to_string()),
        models: vec![enabled.clone(), stock.clone()],
    };
    cache.store(&entry).await.unwrap();
    assert_eq!(
        cache.load(&crate::client_version_to_whole()).await.unwrap(),
        Some(entry)
    );
    let endpoint = TestModelsEndpoint::new(Vec::new());
    let manager = OpenAiModelsManager::new_with_cache(
        cache,
        endpoint.clone(),
        Some(AuthManager::from_auth_for_testing(
            CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        )),
    );
    let catalog = manager
        .raw_model_catalog(RefreshStrategy::Offline, DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    assert_eq!(catalog.models, vec![enabled.clone(), stock.clone()]);
    assert_eq!(endpoint.fetch_count(), 0);
    let config = ModelsManagerConfig::default();
    assert_eq!(
        manager.get_model_info(&enabled.slug, &config).await,
        enabled
    );
    assert_eq!(manager.get_model_info(&stock.slug, &config).await, stock);
}
