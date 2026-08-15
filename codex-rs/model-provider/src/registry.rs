use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use codex_http_client::HttpClientFactory;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;

use crate::SharedModelProvider;

const PROVIDER_MODELS_DIR: &str = "model-providers";

/// One explicit association from a stable provider ID to its runtime Adapter.
#[derive(Clone, Debug)]
pub struct ProviderRegistration {
    picker_label: String,
    runtime: ResolvedProviderRuntime,
}

impl ProviderRegistration {
    /// Creates a Registration whose model catalog is produced by the same Adapter.
    pub fn new(
        id: impl Into<String>,
        picker_label: impl Into<String>,
        provider: SharedModelProvider,
        models_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> Self {
        Self {
            picker_label: picker_label.into(),
            runtime: ResolvedProviderRuntime::from_provider(
                id,
                provider,
                models_home,
                config_model_catalog,
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProviderSelection {
    pub model: String,
    pub provider_id: String,
}

/// One immutable Provider runtime resolved for a Session.
#[derive(Clone, Debug)]
pub struct ResolvedProviderRuntime {
    provider_id: String,
    provider: SharedModelProvider,
    models_manager: SharedModelsManager,
}

impl ResolvedProviderRuntime {
    /// Creates an atomic runtime from one Adapter and its own Models Manager.
    fn from_provider(
        provider_id: impl Into<String>,
        provider: SharedModelProvider,
        models_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> Self {
        let models_manager = provider.models_manager(models_home, config_model_catalog);
        Self {
            provider_id: provider_id.into(),
            provider,
            models_manager,
        }
    }

    /// Returns the stable Provider Registration ID.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Returns the Provider Adapter used for request execution.
    pub fn provider(&self) -> SharedModelProvider {
        self.provider.clone()
    }

    /// Returns the Models Manager created by the same Provider Adapter.
    pub fn models_manager(&self) -> SharedModelsManager {
        self.models_manager.clone()
    }
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
    unavailable_providers: BTreeSet<String>,
}

/// Process-owned Provider Registrations and their isolated model catalogs.
#[derive(Debug)]
pub struct ModelProviderRegistry {
    registrations: BTreeMap<String, ProviderRegistration>,
    registration_order: Vec<String>,
    default_provider_id: String,
}

impl ModelProviderRegistry {
    pub fn new(
        registrations: impl IntoIterator<Item = ProviderRegistration>,
        default_provider_id: &str,
    ) -> CodexResult<Self> {
        let mut registered = BTreeMap::new();
        let mut registration_order = Vec::new();
        for registration in registrations {
            let id = registration.runtime.provider_id.clone();
            if registered.insert(id.clone(), registration).is_some() {
                return Err(CodexErr::InvalidRequest(format!(
                    "model provider `{id}` is registered more than once"
                )));
            }
            registration_order.push(id);
        }
        if !registered.contains_key(default_provider_id) {
            return Err(CodexErr::InvalidRequest(format!(
                "default model provider `{default_provider_id}` is not registered"
            )));
        }
        Ok(Self {
            registrations: registered,
            registration_order,
            default_provider_id: default_provider_id.to_string(),
        })
    }

    /// Returns the model manager owned by the default Provider runtime.
    pub fn default_models_manager(&self) -> SharedModelsManager {
        self.registrations
            .get(&self.default_provider_id)
            .expect("registry construction validates the default provider")
            .runtime
            .models_manager()
    }

    pub fn single(
        provider_id: impl Into<String>,
        provider: SharedModelProvider,
        models_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> Self {
        let id = provider_id.into();
        let picker_label = provider.info().name.clone();
        Self::new(
            [ProviderRegistration::new(
                id.clone(),
                picker_label,
                provider,
                models_home,
                config_model_catalog,
            )],
            &id,
        )
        .expect("single provider registration must contain its default")
    }

    pub fn requires_bound_history(&self) -> bool {
        self.registrations.len() > 1
    }

    pub fn default_thread_provider_filter(&self) -> Option<Vec<String>> {
        (self.registrations.len() == 1).then(|| vec![self.default_provider_id.clone()])
    }

    pub fn requires_binding_resolution(&self, bound_provider_id: &str) -> bool {
        self.registrations.len() > 1 || bound_provider_id != self.default_provider_id
    }

    pub fn resolve_runtime(&self, provider_id: &str) -> CodexResult<ResolvedProviderRuntime> {
        let registration = self.require_registration(provider_id)?;
        Ok(registration.runtime.clone())
    }

    pub async fn list_models(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<Vec<ModelPreset>> {
        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        if catalog.providers.is_empty() && !catalog.unavailable_providers.is_empty() {
            return Err(authority_unavailable_for(
                catalog.unavailable_providers.iter().map(String::as_str),
            ));
        }
        let mut models = Vec::new();
        for provider_catalog in catalog.providers {
            let registration = self
                .registrations
                .get(&provider_catalog.provider_id)
                .expect("catalog provider must be registered");
            for mut model in provider_catalog.models {
                if self.registrations.len() > 1 {
                    model.display_name =
                        format!("{} · {}", registration.picker_label, model.display_name);
                }
                models.push(model);
            }
        }
        Ok(models)
    }

    /// Resolve an explicit new-thread selection through the unified catalog.
    ///
    /// `None` means the caller supplied neither field, so normal config resolution remains
    /// authoritative.
    pub async fn resolve_new_thread_selection(
        &self,
        requested_model: Option<&str>,
        requested_provider_id: Option<&str>,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<Option<ResolvedProviderSelection>> {
        if self.registrations.len() == 1 {
            return Ok(None);
        }
        if requested_model.is_none() && requested_provider_id.is_none() {
            return Ok(None);
        }
        if let Some(provider_id) = requested_provider_id {
            self.require_registration(provider_id)?;
        }

        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        match (requested_model, requested_provider_id) {
            (Some(model), provider_id) => {
                if let Some(provider_id) = provider_id
                    && catalog.unavailable_providers.contains(provider_id)
                {
                    return Err(authority_unavailable(provider_id));
                }
                let owner = catalog.owners.get(model).ok_or_else(|| {
                    if provider_id.is_some() || catalog.unavailable_providers.is_empty() {
                        model_unavailable(provider_id, model)
                    } else {
                        authority_unavailable_for(
                            catalog.unavailable_providers.iter().map(String::as_str),
                        )
                    }
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
                if catalog.unavailable_providers.contains(provider_id) {
                    return Err(authority_unavailable(provider_id));
                }
                let provider_catalog = catalog
                    .providers
                    .iter()
                    .find(|catalog| catalog.provider_id == provider_id)
                    .expect("available registered provider must have a catalog");
                let model = provider_catalog
                    .models
                    .iter()
                    .find(|model| model.is_default)
                    .or_else(|| provider_catalog.models.first())
                    .ok_or_else(|| model_unavailable(Some(provider_id), "<default>"))?;
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
        if !self.requires_binding_resolution(bound_provider_id) {
            return Ok(());
        }
        self.require_registration(bound_provider_id)?;
        let catalog = self
            .load_unified_catalog(refresh_strategy, http_client_factory)
            .await?;
        if catalog.unavailable_providers.contains(bound_provider_id) {
            return Err(authority_unavailable(bound_provider_id));
        }
        let owner = catalog
            .owners
            .get(requested_model)
            .ok_or_else(|| model_unavailable(Some(bound_provider_id), requested_model))?;
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
        if !self.requires_binding_resolution(bound_provider_id)
            && requested_provider_id.is_none_or(|provider_id| provider_id == bound_provider_id)
        {
            return Ok(None);
        }
        self.require_registration(bound_provider_id)?;
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
            self.validate_bound_model(
                bound_provider_id,
                persisted_model,
                refresh_strategy,
                http_client_factory,
            )
            .await?;
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

    fn require_registration(&self, provider_id: &str) -> CodexResult<&ProviderRegistration> {
        self.registrations.get(provider_id).ok_or_else(|| {
            CodexErr::InvalidRequest(format!(
                "ProviderUnavailable: model provider `{provider_id}` is not registered"
            ))
        })
    }

    async fn load_unified_catalog(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<UnifiedModelCatalog> {
        let mut providers = Vec::with_capacity(self.registrations.len());
        let mut owners = HashMap::<String, String>::new();
        let mut unavailable_providers = BTreeSet::new();
        for provider_id in &self.registration_order {
            let registration = self
                .registrations
                .get(provider_id)
                .expect("registration order must reference a registered provider");
            let manager = &registration.runtime.models_manager;
            let models = match manager
                .load_model_catalog(refresh_strategy, http_client_factory.clone())
                .await
            {
                Ok(catalog) => manager.build_available_models(catalog.models),
                Err(_) => {
                    unavailable_providers.insert(provider_id.clone());
                    continue;
                }
            };
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
        Ok(UnifiedModelCatalog {
            providers,
            owners,
            unavailable_providers,
        })
    }
}

fn authority_unavailable(provider_id: &str) -> CodexErr {
    CodexErr::InvalidRequest(format!(
        "AuthorityUnavailable: model catalog for provider `{provider_id}` is unavailable"
    ))
}

fn authority_unavailable_for<'a>(provider_ids: impl IntoIterator<Item = &'a str>) -> CodexErr {
    let provider_ids = provider_ids.into_iter().collect::<Vec<_>>().join("`, `");
    CodexErr::InvalidRequest(format!(
        "AuthorityUnavailable: model catalog for provider(s) `{provider_ids}` is unavailable"
    ))
}

fn model_unavailable(provider_id: Option<&str>, model: &str) -> CodexErr {
    match provider_id {
        Some(provider_id) => CodexErr::InvalidRequest(format!(
            "ModelUnavailable: model `{model}` is not present in the successful catalog for provider `{provider_id}`"
        )),
        None => CodexErr::InvalidRequest(format!(
            "ModelUnavailable: model `{model}` is not present in the successful provider catalogs"
        )),
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

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
