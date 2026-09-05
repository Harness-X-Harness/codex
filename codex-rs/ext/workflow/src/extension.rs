use std::sync::Arc;

use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;

use crate::service::WorkflowService;

/// Host `goal_host` gate for the independent `/workflow` layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkflowExtensionConfig {
    pub enabled: bool,
}

struct WorkflowExtension<C> {
    service: Arc<WorkflowService>,
    workflow_config: Arc<dyn Fn(&C) -> WorkflowExtensionConfig + Send + Sync>,
}

impl<C> ThreadLifecycleContributor<C> for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let mut config = (self.workflow_config)(input.config);
            if matches!(input.session_source, SessionSource::Internal(_)) {
                config.enabled = false;
            }
            input.thread_store.insert(config);
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let enabled = input
                .thread_store
                .get::<WorkflowExtensionConfig>()
                .is_some_and(|config| config.enabled);
            if !enabled {
                return;
            }
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            if let Err(err) = self.service.continue_if_idle(thread_id).await {
                tracing::warn!(
                    "failed to continue active workflow for idle thread {thread_id}: {err}"
                );
            }
        })
    }
}

impl<C> ConfigContributor<C> for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &C,
        new_config: &C,
    ) {
        thread_store.insert((self.workflow_config)(new_config));
    }
}

/// Registers the independent workflow engine. Enabled when `goal_host` is on.
pub fn install<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    service: Arc<WorkflowService>,
    workflow_config: impl Fn(&C) -> WorkflowExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(WorkflowExtension {
        service,
        workflow_config: Arc::new(workflow_config),
    });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension);
}
