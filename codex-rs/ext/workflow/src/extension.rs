use std::sync::Arc;

use codex_core::TurnStartOptions;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadResumeInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnItemContributor;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TurnAbortReason;

use crate::engine::truncate_workflow_reply;
use crate::journal::HOST_ERROR_TURN_CANCELLED;
use crate::journal::HOST_ERROR_TURN_ERRORED;
use crate::journal::HostCallResult;
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

    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
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
            if let Err(err) = self.service.restore_occupancy(thread_id).await {
                tracing::warn!("failed to restore workflow occupancy for {thread_id}: {err}");
            }
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

impl<C> TurnLifecycleContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !workflow_how_enabled(input.thread_store) || !workflow_how_turn(input.turn_store) {
                return;
            }
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let outcome = if input.turn_store.get::<WorkflowTurnFailure>().is_some() {
                self.service
                    .finish_yield_turn_with_result(
                        thread_id,
                        HostCallResult::failure(HOST_ERROR_TURN_ERRORED),
                    )
                    .await
            } else {
                let reply = input
                    .turn_store
                    .get::<WorkflowTurnReply>()
                    .map(|reply| reply.0.clone())
                    .unwrap_or_default();
                self.service
                    .finish_yield_turn_with_result(thread_id, HostCallResult::success(reply))
                    .await
            };
            if let Err(err) = outcome {
                tracing::warn!("failed to host-resume workflow after yield for {thread_id}: {err}");
            }
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !workflow_how_enabled(input.thread_store) || !workflow_how_turn(input.turn_store) {
                return;
            }
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let outcome = match input.reason {
                TurnAbortReason::Interrupted | TurnAbortReason::BudgetLimited => self
                    .service
                    .finish_yield_turn_with_result(
                        thread_id,
                        HostCallResult::failure(HOST_ERROR_TURN_CANCELLED),
                    )
                    .await
                    .map(|_| ()),
                // Another turn replaced this yield. Pause so the run does not
                // stay active with a started yield that can never finish.
                TurnAbortReason::Replaced | TurnAbortReason::ReviewEnded => {
                    self.service.stop_run(thread_id).await.map(|_| ())
                }
            };
            if let Err(err) = outcome {
                tracing::warn!("failed to host-resume workflow after abort for {thread_id}: {err}");
            }
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !workflow_how_enabled(input.thread_store) || !workflow_how_turn(input.turn_store) {
                return;
            }
            input.turn_store.insert(WorkflowTurnFailure);
        })
    }
}

struct WorkflowTurnFailure;

fn workflow_how_enabled(thread_store: &ExtensionData) -> bool {
    thread_store
        .get::<WorkflowExtensionConfig>()
        .is_some_and(|config| config.enabled)
}

fn workflow_how_turn(turn_store: &ExtensionData) -> bool {
    turn_store
        .get::<TurnStartOptions>()
        .is_some_and(|options| options.turn_trigger.as_deref() == Some("workflow"))
}

struct WorkflowTurnReply(String);

impl<C> TurnItemContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a mut TurnItem,
    ) -> ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if workflow_how_turn(turn_store)
                && let TurnItem::AgentMessage(message) = &*item
            {
                let text: String = message
                    .content
                    .iter()
                    .map(|entry| match entry {
                        AgentMessageContent::Text { text } => text.as_str(),
                    })
                    .collect();
                turn_store.insert(WorkflowTurnReply(truncate_workflow_reply(&text)));
            }
            Ok(())
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
    registry.turn_lifecycle_contributor(extension.clone());
    registry.turn_item_contributor(extension.clone());
    registry.config_contributor(extension);
}
