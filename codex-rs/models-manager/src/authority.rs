use super::*;

/// Provider-owned resolution result for one requested model.
#[derive(Debug, Clone)]
pub enum ModelResolution {
    /// The requested model and picker metadata resolved from one catalog snapshot.
    Resolved {
        model_info: ModelInfo,
        available_models: Vec<ModelPreset>,
    },
    /// The authoritative catalog was loaded successfully and omits the requested model.
    Unavailable { model: String },
}

/// Model selection policy evaluated against one catalog generation.
#[derive(Debug, Clone, Copy)]
pub enum ModelSelection<'a> {
    Exact(&'a str),
    ProviderDefault,
    PreferRequested(&'a str),
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

    pub(super) async fn resolve_model_profile(
        &self,
        selection: ModelSelection<'_>,
        config: &ModelsManagerConfig,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<ModelResolution> {
        let catalog = self
            .load_catalog(refresh_strategy, http_client_factory)
            .await?;
        let available_models = self.build_available_models(catalog.models.clone());
        let model = select_model(
            selection,
            &available_models,
            self.endpoint_client.remote_catalog_is_authoritative(),
        );
        if self.endpoint_client.remote_catalog_is_authoritative() {
            return Ok(
                match catalog.models.iter().find(|candidate| {
                    candidate.slug == model.as_str() && candidate.supported_in_api
                }) {
                    Some(candidate) => ModelResolution::Resolved {
                        model_info: model_info::with_config_overrides(candidate.clone(), config),
                        available_models,
                    },
                    None => ModelResolution::Unavailable { model },
                },
            );
        }
        Ok(ModelResolution::Resolved {
            model_info: construct_model_info_from_candidates(&model, &catalog.models, config),
            available_models,
        })
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

    pub(super) fn resolve_model_profile(
        &self,
        selection: ModelSelection<'_>,
        config: &ModelsManagerConfig,
    ) -> ModelResolution {
        let available_models = self.build_available_models(self.remote_models.clone());
        let model = select_model(
            selection,
            &available_models,
            self.constrains_model_availability,
        );
        if self.constrains_model_availability {
            return match self
                .remote_models
                .iter()
                .find(|candidate| candidate.slug == model.as_str() && candidate.supported_in_api)
            {
                Some(candidate) => ModelResolution::Resolved {
                    model_info: model_info::with_config_overrides(candidate.clone(), config),
                    available_models,
                },
                None => ModelResolution::Unavailable { model },
            };
        }
        ModelResolution::Resolved {
            model_info: construct_model_info_from_candidates(&model, &self.remote_models, config),
            available_models,
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

    fn resolve_model_profile<'a>(
        &'a self,
        selection: ModelSelection<'a>,
        config: &'a ModelsManagerConfig,
        _refresh_strategy: RefreshStrategy,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'a, CoreResult<ModelResolution>> {
        Box::pin(async move {
            Ok(StaticModelsManager::resolve_model_profile(
                self, selection, config,
            ))
        })
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

fn select_model(
    selection: ModelSelection<'_>,
    available_models: &[ModelPreset],
    constrains_model_availability: bool,
) -> String {
    match selection {
        ModelSelection::Exact(model) => model.to_string(),
        ModelSelection::ProviderDefault => default_model_from_available(available_models),
        ModelSelection::PreferRequested(model)
            if !constrains_model_availability
                || requested_model_is_available(Some(model), available_models) =>
        {
            model.to_string()
        }
        ModelSelection::PreferRequested(_) => default_model_from_available(available_models),
    }
}
