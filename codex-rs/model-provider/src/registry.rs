use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_http_client::HttpClientFactory;
use codex_login::AuthManager;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::openai_models::ModelPreset;

use crate::SharedModelProvider;
use crate::create_model_provider;

const PROVIDER_MODELS_DIR: &str = "model-providers";

#[derive(Clone, Debug)]
struct ProviderProfile {
    id: String,
    display_name: String,
    provider: SharedModelProvider,
    models_manager: SharedModelsManager,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProviderSelection {
    pub model: String,
    pub provider_id: String,
}

#[derive(Debug)]
struct ProviderCatalog {
    provider_id: String,
    models: Vec<ModelPreset>,
}

#[derive(Debug)]
struct UnifiedModelCatalog {
    providers: Vec<ProviderCatalog>,
    owners: HashMap<String, String>,
}

/// Process-owned provider profiles and their isolated model catalogs.
#[derive(Debug)]
pub struct ModelProviderRegistry {
    profiles: BTreeMap<String, ProviderProfile>,
    selectable_ids: Vec<String>,
    federated: bool,
}

impl ModelProviderRegistry {
    pub fn new(
        providers: &HashMap<String, ModelProviderInfo>,
        default_provider_id: &str,
        codex_home: &Path,
        default_models_manager: SharedModelsManager,
        auth_manager: Arc<AuthManager>,
    ) -> CodexResult<Self> {
        let federated = providers
            .values()
            .any(|provider| provider.wire_api == WireApi::GrokResponses);
        let mut profiles = BTreeMap::new();
        for (id, info) in providers {
            let provider = create_model_provider(info.clone(), Some(Arc::clone(&auth_manager)));
            let models_manager = if id == default_provider_id {
                Arc::clone(&default_models_manager)
            } else {
                provider.models_manager(
                    provider_models_home(codex_home, id),
                    /*config_model_catalog*/ None,
                )
            };
            profiles.insert(
                id.clone(),
                ProviderProfile {
                    id: id.clone(),
                    display_name: info.name.clone(),
                    provider,
                    models_manager,
                },
            );
        }
        if !profiles.contains_key(default_provider_id) {
            return Err(CodexErr::InvalidRequest(format!(
                "default model provider `{default_provider_id}` is not registered"
            )));
        }

        let selectable_ids = selectable_provider_ids(&profiles, default_provider_id, federated);
        Ok(Self {
            profiles,
            selectable_ids,
            federated,
        })
    }

    pub fn single(
        provider_id: impl Into<String>,
        provider: SharedModelProvider,
        models_manager: SharedModelsManager,
    ) -> Self {
        let id = provider_id.into();
        let profile = ProviderProfile {
            id: id.clone(),
            display_name: provider.info().name.clone(),
            provider,
            models_manager,
        };
        Self {
            profiles: BTreeMap::from([(id.clone(), profile)]),
            selectable_ids: vec![id],
            federated: false,
        }
    }

    pub fn is_federated(&self) -> bool {
        self.federated
    }

    pub fn models_manager(&self, provider_id: &str) -> Option<SharedModelsManager> {
        self.profiles
            .get(provider_id)
            .map(|profile| Arc::clone(&profile.models_manager))
    }

    pub fn provider(&self, provider_id: &str) -> Option<SharedModelProvider> {
        self.profiles
            .get(provider_id)
            .map(|profile| Arc::clone(&profile.provider))
    }

    pub async fn list_models(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<Vec<ModelPreset>> {
        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        let mut models = Vec::new();
        for provider_catalog in catalog.providers {
            let profile = self
                .profiles
                .get(&provider_catalog.provider_id)
                .expect("selectable provider must be registered");
            for mut model in provider_catalog.models {
                if self.federated {
                    model.display_name =
                        format!("{} · {}", profile_picker_label(profile), model.display_name);
                }
                models.push(model);
            }
        }
        Ok(models)
    }

    /// Resolve an explicit new-thread selection through the unified catalog.
    ///
    /// `None` means the caller supplied neither field, or explicitly selected a legacy provider
    /// that is outside the OpenAI/Grok unified catalog. In both cases normal config resolution
    /// remains authoritative.
    pub async fn resolve_new_thread_selection(
        &self,
        requested_model: Option<&str>,
        requested_provider_id: Option<&str>,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<Option<ResolvedProviderSelection>> {
        if !self.federated || requested_model.is_none() && requested_provider_id.is_none() {
            return Ok(None);
        }
        if requested_provider_id.is_some_and(|provider_id| !self.is_selectable(provider_id)) {
            return Ok(None);
        }

        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        match (requested_model, requested_provider_id) {
            (Some(model), provider_id) => {
                let owner = catalog.owners.get(model).ok_or_else(|| {
                    CodexErr::InvalidRequest(format!(
                        "model `{model}` is not present in the unified provider catalog"
                    ))
                })?;
                if let Some(provider_id) = provider_id
                    && provider_id != owner
                {
                    return Err(CodexErr::InvalidRequest(format!(
                        "model `{model}` belongs to provider `{owner}`, not `{provider_id}`"
                    )));
                }
                Ok(Some(ResolvedProviderSelection {
                    model: model.to_string(),
                    provider_id: owner.clone(),
                }))
            }
            (None, Some(provider_id)) => {
                let provider_catalog = catalog
                    .providers
                    .iter()
                    .find(|catalog| catalog.provider_id == provider_id)
                    .expect("selectable provider must have a catalog");
                let model = provider_catalog
                    .models
                    .iter()
                    .find(|model| model.is_default)
                    .or_else(|| provider_catalog.models.first())
                    .expect("provider catalog must not be empty");
                Ok(Some(ResolvedProviderSelection {
                    model: model.model.clone(),
                    provider_id: provider_id.to_string(),
                }))
            }
            (None, None) => Ok(None),
        }
    }

    pub async fn validate_bound_model(
        &self,
        bound_provider_id: &str,
        requested_model: &str,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<()> {
        if !self.federated || !self.is_selectable(bound_provider_id) {
            return Ok(());
        }
        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        let owner = catalog.owners.get(requested_model).ok_or_else(|| {
            CodexErr::InvalidRequest(format!(
                "model `{requested_model}` is not present in the unified provider catalog"
            ))
        })?;
        if owner != bound_provider_id {
            return Err(CodexErr::InvalidRequest(format!(
                "thread is bound to provider `{bound_provider_id}`, but model `{requested_model}` belongs to `{owner}`; start a new thread to use another provider"
            )));
        }
        Ok(())
    }

    /// Resolve an existing thread without changing its provider binding.
    ///
    /// A requested model may replace the persisted model only when both models belong to the
    /// bound provider. A missing persisted model falls back to that provider's catalog default.
    pub async fn resolve_existing_thread_selection(
        &self,
        bound_provider_id: &str,
        persisted_model: Option<&str>,
        requested_model: Option<&str>,
        requested_provider_id: Option<&str>,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<Option<ResolvedProviderSelection>> {
        if !self.federated || !self.is_selectable(bound_provider_id) {
            return Ok(None);
        }
        if let Some(requested_provider_id) = requested_provider_id
            && requested_provider_id != bound_provider_id
        {
            return Err(CodexErr::InvalidRequest(format!(
                "thread is bound to provider `{bound_provider_id}`, not `{requested_provider_id}`; start a new thread to use another provider"
            )));
        }

        if let Some(requested_model) = requested_model {
            self.validate_bound_model(
                bound_provider_id,
                requested_model,
                refresh_strategy,
                http_client_factory,
            )
            .await?;
            return Ok(Some(ResolvedProviderSelection {
                model: requested_model.to_string(),
                provider_id: bound_provider_id.to_string(),
            }));
        }
        if let Some(persisted_model) = persisted_model {
            return Ok(Some(ResolvedProviderSelection {
                model: persisted_model.to_string(),
                provider_id: bound_provider_id.to_string(),
            }));
        }

        self.resolve_new_thread_selection(
            /*requested_model*/ None,
            Some(bound_provider_id),
            refresh_strategy,
            http_client_factory,
        )
        .await
    }

    fn is_selectable(&self, provider_id: &str) -> bool {
        self.selectable_ids
            .iter()
            .any(|selectable_id| selectable_id == provider_id)
    }

    async fn load_unified_catalog(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<UnifiedModelCatalog> {
        let mut providers = Vec::with_capacity(self.selectable_ids.len());
        let mut owners = HashMap::<String, String>::new();
        for provider_id in &self.selectable_ids {
            let profile = self
                .profiles
                .get(provider_id)
                .expect("selectable provider must be registered");
            let models = profile
                .models_manager
                .list_models(refresh_strategy, http_client_factory.clone())
                .await;
            if models.is_empty() {
                return Err(CodexErr::InvalidRequest(format!(
                    "model provider `{provider_id}` has no available model catalog"
                )));
            }
            for model in &models {
                if let Some(previous_owner) =
                    owners.insert(model.model.clone(), provider_id.clone())
                {
                    return Err(CodexErr::InvalidRequest(format!(
                        "model `{}` is advertised by both `{previous_owner}` and `{provider_id}`",
                        model.model
                    )));
                }
            }
            providers.push(ProviderCatalog {
                provider_id: provider_id.clone(),
                models,
            });
        }
        Ok(UnifiedModelCatalog { providers, owners })
    }
}

/// Returns a stable, collision-free cache root for one provider.
///
/// OpenAI keeps the historical root cache path. Other provider IDs are encoded
/// byte-for-byte into bounded path components, so one provider can never read
/// another provider's model cache.
pub fn provider_models_home(codex_home: &Path, provider_id: &str) -> PathBuf {
    if provider_id == OPENAI_PROVIDER_ID {
        return codex_home.to_path_buf();
    }
    let mut path = codex_home.join(PROVIDER_MODELS_DIR);
    if provider_id.is_empty() {
        path.push("_");
        return path;
    }
    for chunk in provider_id.as_bytes().chunks(32) {
        let mut component = String::with_capacity(chunk.len() * 2);
        for byte in chunk {
            let _ = write!(component, "{byte:02x}");
        }
        path.push(component);
    }
    path
}

fn selectable_provider_ids(
    profiles: &BTreeMap<String, ProviderProfile>,
    default_provider_id: &str,
    federated: bool,
) -> Vec<String> {
    if !federated {
        return vec![default_provider_id.to_string()];
    }
    let mut ids = Vec::new();
    if profiles.contains_key(OPENAI_PROVIDER_ID) {
        ids.push(OPENAI_PROVIDER_ID.to_string());
    }
    if !ids.iter().any(|id| id == default_provider_id) {
        ids.push(default_provider_id.to_string());
    }
    for profile in profiles.values() {
        if profile.provider.info().wire_api == WireApi::GrokResponses
            && !ids.iter().any(|id| id == &profile.id)
        {
            ids.push(profile.id.clone());
        }
    }
    ids
}

fn profile_picker_label(profile: &ProviderProfile) -> &str {
    if profile.id == OPENAI_PROVIDER_ID {
        "ChatGPT"
    } else {
        profile.display_name.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_http_client::OutboundProxyPolicy;
    use codex_login::CodexAuth;
    use codex_models_manager::manager::StaticModelsManager;
    use codex_protocol::openai_models::ModelInfo;
    use codex_protocol::openai_models::ModelsResponse;
    use serde_json::json;

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
    async fn federated_catalog_is_labeled_and_duplicate_slugs_fail_atomically() {
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
        let openai = test_profile(
            OPENAI_PROVIDER_ID,
            "OpenAI",
            WireApi::Responses,
            openai_slugs,
        );
        let grok = test_profile("grok", "Grok", WireApi::GrokResponses, grok_slugs);
        ModelProviderRegistry {
            profiles: BTreeMap::from([
                (OPENAI_PROVIDER_ID.to_string(), openai),
                ("grok".to_string(), grok),
            ]),
            selectable_ids: vec![OPENAI_PROVIDER_ID.to_string(), "grok".to_string()],
            federated: true,
        }
    }

    fn test_profile(
        id: &str,
        display_name: &str,
        wire_api: WireApi,
        slugs: &[&str],
    ) -> ProviderProfile {
        let info = ModelProviderInfo {
            name: display_name.to_string(),
            wire_api,
            ..ModelProviderInfo::default()
        };
        let provider = create_model_provider(info, /*auth_manager*/ None);
        let models_manager: SharedModelsManager = Arc::new(StaticModelsManager::new(
            /*auth_manager*/ None,
            ModelsResponse {
                models: slugs.iter().map(|slug| test_model(slug)).collect(),
            },
        ));
        ProviderProfile {
            id: id.to_string(),
            display_name: display_name.to_string(),
            provider,
            models_manager,
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
}
