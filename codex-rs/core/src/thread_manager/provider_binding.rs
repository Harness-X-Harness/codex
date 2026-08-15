use super::ThreadManagerState;
use crate::config::Config;
use codex_http_client::HttpClientFactory;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_thread_store::ReadThreadParams;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThreadProviderBinding {
    provider_id: String,
    model: Option<String>,
}

impl ThreadManagerState {
    pub(crate) fn provider_binding_enforced(&self) -> bool {
        self.provider_registry.requires_bound_history()
    }

    pub(crate) async fn validate_bound_model(
        &self,
        provider_id: &str,
        model: &str,
        http_client_factory: HttpClientFactory,
    ) -> CodexResult<()> {
        self.provider_registry
            .validate_bound_model(
                provider_id,
                model,
                RefreshStrategy::OnlineIfUncached,
                http_client_factory,
            )
            .await
    }

    /// Apply the source Thread's immutable provider binding to a derived Thread config.
    pub(super) async fn bind_derived_thread_config(
        &self,
        config: &mut Config,
        parent_thread_id: Option<ThreadId>,
        forked_from_thread_id: Option<ThreadId>,
    ) -> CodexResult<()> {
        let fork_binding = match forked_from_thread_id {
            Some(thread_id) => Some(self.thread_provider_binding(thread_id).await?),
            None => None,
        };
        let parent_binding = match parent_thread_id {
            Some(thread_id) if Some(thread_id) != forked_from_thread_id => {
                Some(self.thread_provider_binding(thread_id).await?)
            }
            Some(_) => fork_binding.clone(),
            None => None,
        };
        let source_binding = match (fork_binding, parent_binding) {
            (Some(fork), Some(parent)) => {
                if fork.provider_id != parent.provider_id {
                    return Err(CodexErr::InvalidRequest(format!(
                        "derived thread sources are bound to different providers (`{}` and `{}`); start a new thread instead",
                        fork.provider_id, parent.provider_id
                    )));
                }
                fork
            }
            (Some(binding), None) | (None, Some(binding)) => binding,
            (None, None) => return Ok(()),
        };

        if !self
            .provider_registry
            .requires_binding_resolution(&source_binding.provider_id)
            && config.model_provider_id == source_binding.provider_id
        {
            return Ok(());
        }

        if config.model_provider_id != source_binding.provider_id {
            return Err(CodexErr::InvalidRequest(format!(
                "thread is bound to provider `{}`, not `{}`; start a new thread to use another provider",
                source_binding.provider_id, config.model_provider_id
            )));
        }

        let requested_model = config.model.as_deref().or(source_binding.model.as_deref());
        if let Some(requested_model) = requested_model
            && source_binding.model.as_deref() != Some(requested_model)
        {
            self.validate_bound_model(
                &source_binding.provider_id,
                requested_model,
                config.http_client_factory(),
            )
            .await?;
        }
        if config.model.is_none() {
            config.model = source_binding.model;
        }

        let runtime = self
            .provider_registry
            .resolve_runtime(&source_binding.provider_id)?;
        config.model_provider = runtime.provider().info().clone();
        Ok(())
    }

    async fn thread_provider_binding(
        &self,
        thread_id: ThreadId,
    ) -> CodexResult<ThreadProviderBinding> {
        if let Ok(thread) = self.get_thread(thread_id).await {
            let snapshot = thread.config_snapshot().await;
            return Ok(ThreadProviderBinding {
                provider_id: snapshot.model_provider_id,
                model: Some(snapshot.model),
            });
        }

        let stored = self
            .read_stored_thread(ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await?;
        if stored.model_provider.is_empty() {
            return Err(CodexErr::InvalidRequest(format!(
                "cannot derive thread {thread_id}: its provider binding is missing"
            )));
        }
        Ok(ThreadProviderBinding {
            provider_id: stored.model_provider,
            model: stored.model,
        })
    }
}
