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
#[path = "registry_tests.rs"]
mod tests;
