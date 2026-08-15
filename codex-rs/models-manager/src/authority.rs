use super::*;

/// Provider-owned availability result for one requested model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAvailability {
    /// This catalog supplies metadata but does not constrain accepted model identifiers.
    Unconstrained,
    /// The authoritative catalog contains the requested model.
    Available,
    /// The authoritative catalog was loaded successfully and omits the requested model.
    Unavailable,
}

impl OpenAiModelsManager {
    pub(super) async fn load_catalog(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<ModelsResponse> {
        if let Err(error) = self
            .refresh_available_models(refresh_strategy, &http_client_factory)
            .await
        {
            if self.endpoint_client.remote_catalog_is_authoritative() {
                return Err(error);
            }
            error!("failed to refresh non-authoritative model catalog: {error}");
        }
        Ok(ModelsResponse {
            models: self.remote_models.read().await.clone(),
        })
    }

    pub(super) async fn model_availability(
        &self,
        model: &str,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<ModelAvailability> {
        if !self.endpoint_client.remote_catalog_is_authoritative() {
            return Ok(ModelAvailability::Unconstrained);
        }
        let catalog = self
            .load_catalog(refresh_strategy, http_client_factory)
            .await?;
        Ok(
            if catalog
                .models
                .iter()
                .any(|candidate| candidate.slug == model && candidate.supported_in_api)
            {
                ModelAvailability::Available
            } else {
                ModelAvailability::Unavailable
            },
        )
    }
}

impl StaticModelsManager {
    /// Construct a static model manager from an authoritative catalog.
    pub fn new(auth_manager: Option<Arc<AuthManager>>, model_catalog: ModelsResponse) -> Self {
        Self {
            remote_models: model_catalog.models,
            auth_manager,
            constrains_model_availability: true,
        }
    }

    /// Constructs a static metadata catalog that does not constrain Provider model identifiers.
    pub fn new_unconstrained(
        auth_manager: Option<Arc<AuthManager>>,
        model_catalog: ModelsResponse,
    ) -> Self {
        Self {
            remote_models: model_catalog.models,
            auth_manager,
            constrains_model_availability: false,
        }
    }

    pub(super) fn model_availability(&self, model: &str) -> ModelAvailability {
        if !self.constrains_model_availability {
            return ModelAvailability::Unconstrained;
        }
        if self
            .remote_models
            .iter()
            .any(|candidate| candidate.slug == model && candidate.supported_in_api)
        {
            ModelAvailability::Available
        } else {
            ModelAvailability::Unavailable
        }
    }
}

impl ModelsManager for StaticModelsManager {
    fn load_model_catalog(
        &self,
        _refresh_strategy: RefreshStrategy,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, CoreResult<ModelsResponse>> {
        Box::pin(async move {
            Ok(ModelsResponse {
                models: self.get_remote_models().await,
            })
        })
    }

    fn model_availability<'a>(
        &'a self,
        model: &'a str,
        _refresh_strategy: RefreshStrategy,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'a, CoreResult<ModelAvailability>> {
        Box::pin(async move { Ok(StaticModelsManager::model_availability(self, model)) })
    }

    fn get_default_model<'a>(
        &'a self,
        model: &'a Option<String>,
        allow_provider_model_fallback: bool,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'a, String> {
        Box::pin(
            async move {
                let available_models = self
                    .list_models(refresh_strategy, http_client_factory)
                    .await;
                let requested_model = model.as_deref();

                if allow_provider_model_fallback {
                    if requested_model_is_available(requested_model, &available_models)
                        && let Some(requested_model) = requested_model
                    {
                        return requested_model.to_string();
                    }
                    return default_model_from_available(available_models);
                }

                model
                    .clone()
                    .unwrap_or_else(|| default_model_from_available(available_models))
            }
            .instrument(tracing::info_span!(
                "get_default_model",
                model.provided = model.is_some(),
                allow_provider_model_fallback,
                refresh_strategy = %refresh_strategy
            )),
        )
    }

    fn raw_model_catalog(
        &self,
        _refresh_strategy: RefreshStrategy,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, ModelsResponse> {
        Box::pin(async move {
            ModelsResponse {
                models: self.get_remote_models().await,
            }
        })
    }

    fn get_remote_models(&self) -> ModelsManagerFuture<'_, Vec<ModelInfo>> {
        Box::pin(async { self.remote_models.clone() })
    }

    fn try_get_remote_models(&self) -> Result<Vec<ModelInfo>, TryLockError> {
        Ok(self.remote_models.clone())
    }

    fn auth_manager(&self) -> Option<&AuthManager> {
        self.auth_manager.as_deref()
    }

    fn list_collaboration_modes(&self) -> Vec<CollaborationModeMask> {
        builtin_collaboration_mode_presets()
    }

    fn refresh_if_new_etag(
        &self,
        _etag: String,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, ()> {
        Box::pin(async {})
    }
}
