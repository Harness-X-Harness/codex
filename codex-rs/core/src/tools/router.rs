use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
#[cfg(test)]
use crate::session::turn_context::TurnContext;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::grok_hosted_output::GrokHostedOutput;
use crate::tools::grok_hosted_output::GrokHostedOutputEventPhase;
use crate::tools::grok_hosted_output::classify_grok_hosted_output;
#[cfg(test)]
use crate::tools::handlers::ToolSearchHandlerCache;
use crate::tools::registry::AnyToolResult;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolArgumentDiffConsumer;
use crate::tools::registry::ToolRegistry;
#[cfg(test)]
use crate::tools::spec_plan::finalize_tool_router;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::SearchToolCallParams;
use codex_tools::DiscoverableTool;
use codex_tools::GrokLocalToolInput;
use codex_tools::GrokToolPlan;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

pub use crate::tools::context::ToolCallSource;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub tool_name: ToolName,
    pub call_id: String,
    pub payload: ToolPayload,
    pub encrypted_function_args: Option<Vec<String>>,
}

impl ToolCall {
    pub(crate) fn direct_source(&self) -> ToolCallSource {
        if self.tool_name.namespace.as_deref() == Some("collaboration")
            && matches!(
                self.tool_name.name.as_str(),
                "spawn_agent" | "send_message" | "followup_task"
            )
            && self
                .encrypted_function_args
                .as_ref()
                .is_some_and(Vec::is_empty)
        {
            ToolCallSource::DirectPlaintextMessage
        } else {
            ToolCallSource::Direct
        }
    }
}

pub(crate) fn tool_log_payload<'a>(
    payload: &'a ToolPayload,
    source: &ToolCallSource,
) -> Cow<'a, str> {
    if matches!(source, ToolCallSource::DirectPlaintextMessage) {
        return Cow::Borrowed("[plaintext arguments]");
    }
    payload.log_payload()
}

pub struct ToolRouter {
    registry: ToolRegistry,
    model_visible_specs: Vec<ToolSpec>,
    grok_tool_plan: Option<GrokToolPlan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolSuggestPresentation {
    ListTool,
    RecommendationContext,
}

#[derive(Clone, Debug)]
pub(crate) struct ToolSuggestCandidates {
    pub(crate) tools: Vec<DiscoverableTool>,
    pub(crate) presentation: ToolSuggestPresentation,
}

impl ToolRouter {
    #[cfg(test)]
    pub(crate) fn from_registry(
        turn_context: &TurnContext,
        registry: ToolRegistry,
        hosted_specs: Vec<ToolSpec>,
        tool_search_handler_cache: &ToolSearchHandlerCache,
    ) -> Self {
        finalize_tool_router(
            turn_context,
            registry,
            hosted_specs,
            tool_search_handler_cache,
            &[],
            &Default::default(),
        )
        .expect("test tool registry should not contain duplicate tools")
    }

    pub(crate) fn from_parts(registry: ToolRegistry, model_visible_specs: Vec<ToolSpec>) -> Self {
        Self {
            registry,
            model_visible_specs,
            grok_tool_plan: None,
        }
    }

    pub(crate) fn from_grok_plan(registry: ToolRegistry, grok_tool_plan: GrokToolPlan) -> Self {
        Self {
            registry,
            model_visible_specs: grok_tool_plan.declarations.clone(),
            grok_tool_plan: Some(grok_tool_plan),
        }
    }

    pub(crate) fn model_visible_specs(&self) -> Vec<ToolSpec> {
        self.model_visible_specs.clone()
    }

    pub(crate) fn deferred_tool_namespaces(&self) -> BTreeMap<String, String> {
        self.registry.deferred_tool_namespaces()
    }

    #[cfg(test)]
    pub(crate) fn registered_tool_names_for_test(&self) -> Vec<ToolName> {
        self.registry.tool_names_for_test()
    }

    #[cfg(test)]
    pub(crate) fn tool_exposure_for_test(
        &self,
        name: &ToolName,
    ) -> Option<crate::tools::registry::ToolExposure> {
        self.registry.tool_exposure(name)
    }

    pub(crate) fn create_diff_consumer(
        &self,
        tool_name: &ToolName,
    ) -> Option<Box<dyn ToolArgumentDiffConsumer>> {
        self.registry.create_diff_consumer(tool_name)
    }

    pub(crate) fn classify_grok_hosted_output<'a>(
        &self,
        item: &'a ResponseItem,
        phase: GrokHostedOutputEventPhase,
    ) -> Result<GrokHostedOutput<'a>, FunctionCallError> {
        classify_grok_hosted_output(self.grok_tool_plan.as_ref(), item, phase)
    }

    pub(crate) fn defers_local_tool_dispatch_until_response_validated(&self) -> bool {
        self.grok_tool_plan.is_some()
    }

    pub fn tool_supports_parallel(&self, call: &ToolCall) -> bool {
        self.registry
            .supports_parallel_tool_calls(&call.tool_name)
            .unwrap_or(false)
    }

    pub(crate) fn projects_custom_call_as_function(&self, call: &ToolCall) -> bool {
        self.grok_tool_plan.is_some() && matches!(call.payload, ToolPayload::Custom { .. })
    }

    pub(crate) fn tool_runtime(&self, call: &ToolCall) -> Option<Arc<dyn CoreToolRuntime>> {
        self.registry.tool(&call.tool_name)
    }

    pub fn tool_waits_for_runtime_cancellation(&self, call: &ToolCall) -> bool {
        self.registry
            .waits_for_runtime_cancellation(&call.tool_name)
            .unwrap_or(false)
    }

    #[instrument(level = "trace", skip_all, err)]
    pub fn build_tool_call(item: ResponseItem) -> Result<Option<ToolCall>, FunctionCallError> {
        match item {
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                ..
            } => {
                let tool_name = ToolName::new(namespace, name).with_default_namespace();
                Ok(Some(ToolCall {
                    tool_name,
                    call_id,
                    payload: ToolPayload::Function { arguments },
                    encrypted_function_args,
                }))
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                execution,
                arguments,
                ..
            } if execution == "client" => {
                let arguments: SearchToolCallParams =
                    serde_json::from_value(arguments).map_err(|err| {
                        FunctionCallError::RespondToModel(format!(
                            "failed to parse tool_search arguments: {err}"
                        ))
                    })?;
                Ok(Some(ToolCall {
                    tool_name: ToolName::plain("tool_search"),
                    call_id,
                    payload: ToolPayload::ToolSearch { arguments },
                    encrypted_function_args: None,
                }))
            }
            ResponseItem::ToolSearchCall { .. } => Ok(None),
            ResponseItem::CustomToolCall {
                name,
                namespace,
                input,
                call_id,
                ..
            } => Ok(Some(ToolCall {
                tool_name: ToolName::new(namespace, name).with_default_namespace(),
                call_id,
                payload: ToolPayload::Custom { input },
                encrypted_function_args: None,
            })),
            _ => Ok(None),
        }
    }

    #[instrument(level = "trace", skip_all, err)]
    pub fn route_tool_call(
        &self,
        item: ResponseItem,
    ) -> Result<Option<ToolCall>, FunctionCallError> {
        let Some(plan) = &self.grok_tool_plan else {
            return Self::build_tool_call(item);
        };
        if matches!(item, ResponseItem::CustomToolCall { .. }) {
            return match self
                .classify_grok_hosted_output(&item, GrokHostedOutputEventPhase::Done)?
            {
                GrokHostedOutput::Hosted { .. } => Ok(None),
                GrokHostedOutput::UnknownCustom { reason, .. } => {
                    Err(FunctionCallError::Fatal(reason))
                }
                GrokHostedOutput::Ordinary => Err(FunctionCallError::Fatal(
                    "Grok custom output did not resolve under the active Tool Plan".to_string(),
                )),
            };
        }
        let ResponseItem::FunctionCall {
            name,
            namespace,
            arguments,
            encrypted_function_args,
            call_id,
            ..
        } = item
        else {
            return Self::build_tool_call(item);
        };
        if namespace.as_deref().is_some_and(|value| !value.is_empty()) {
            return Err(FunctionCallError::RespondToModel(
                "Grok function_call unexpectedly included a namespace".to_string(),
            ));
        }
        let decoded = plan
            .decode_local_function_call(&name, &arguments)
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "Grok returned undeclared local function `{name}`"
                ))
            })?;
        let payload = match decoded.input {
            GrokLocalToolInput::FunctionArguments(arguments) => ToolPayload::Function { arguments },
            GrokLocalToolInput::Freeform(input) => ToolPayload::Custom { input },
            GrokLocalToolInput::ToolSearchArguments(arguments) => {
                let arguments: SearchToolCallParams =
                    serde_json::from_str(&arguments).map_err(|err| {
                        FunctionCallError::RespondToModel(format!(
                            "failed to parse tool_search arguments: {err}"
                        ))
                    })?;
                ToolPayload::ToolSearch { arguments }
            }
        };
        // Grok function arguments are plaintext. Normalize the missing OpenAI
        // marker into Codex's existing plaintext representation before local dispatch.
        let encrypted_function_args = Some(encrypted_function_args.unwrap_or_default());
        Ok(Some(ToolCall {
            tool_name: decoded.canonical_identity,
            call_id,
            payload,
            encrypted_function_args,
        }))
    }

    pub(crate) fn tool_call_history_item(
        &self,
        item: &ResponseItem,
        call: &ToolCall,
    ) -> Result<Option<ResponseItem>, FunctionCallError> {
        let ResponseItem::FunctionCall {
            id,
            internal_chat_message_metadata_passthrough,
            ..
        } = item
        else {
            return Ok(None);
        };
        if self.grok_tool_plan.is_none() || !matches!(call.payload, ToolPayload::ToolSearch { .. })
        {
            return Ok(None);
        }
        let ToolPayload::ToolSearch { arguments } = &call.payload else {
            unreachable!("guarded by the ToolSearch payload check");
        };
        let arguments = serde_json::to_value(arguments).map_err(|error| {
            FunctionCallError::Fatal(format!(
                "failed to preserve canonical tool_search history: {error}"
            ))
        })?;
        Ok(Some(ResponseItem::ToolSearchCall {
            id: id.clone(),
            call_id: Some(call.call_id.clone()),
            status: Some("completed".to_string()),
            execution: "client".to_string(),
            arguments,
            internal_chat_message_metadata_passthrough: internal_chat_message_metadata_passthrough
                .clone(),
        }))
    }

    #[allow(dead_code)]
    #[instrument(level = "trace", skip_all, err)]
    pub async fn dispatch_tool_call_with_code_mode_result(
        &self,
        session: Arc<Session>,
        step_context: Arc<StepContext>,
        cancellation_token: CancellationToken,
        tracker: SharedTurnDiffTracker,
        call: ToolCall,
        source: ToolCallSource,
    ) -> Result<AnyToolResult, FunctionCallError> {
        self.dispatch_tool_call_with_code_mode_result_inner(
            session,
            step_context,
            cancellation_token,
            tracker,
            call,
            source,
            /*terminal_outcome_reached*/ None,
        )
        .await
    }

    #[instrument(level = "trace", skip_all, err)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn dispatch_tool_call_with_terminal_outcome(
        &self,
        session: Arc<Session>,
        step_context: Arc<StepContext>,
        cancellation_token: CancellationToken,
        tracker: SharedTurnDiffTracker,
        call: ToolCall,
        source: ToolCallSource,
        terminal_outcome_reached: Arc<AtomicBool>,
    ) -> Result<AnyToolResult, FunctionCallError> {
        self.dispatch_tool_call_with_code_mode_result_inner(
            session,
            step_context,
            cancellation_token,
            tracker,
            call,
            source,
            Some(terminal_outcome_reached),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_tool_call_with_code_mode_result_inner(
        &self,
        session: Arc<Session>,
        step_context: Arc<StepContext>,
        cancellation_token: CancellationToken,
        tracker: SharedTurnDiffTracker,
        call: ToolCall,
        source: ToolCallSource,
        terminal_outcome_reached: Option<Arc<AtomicBool>>,
    ) -> Result<AnyToolResult, FunctionCallError> {
        let ToolCall {
            tool_name,
            call_id,
            payload,
            ..
        } = call;

        // Keep the legacy ToolInvocation.turn field tied to the same request state until handlers migrate.
        let turn = Arc::clone(&step_context.turn);
        let invocation = ToolInvocation {
            session,
            turn,
            step_context,
            cancellation_token,
            tracker,
            call_id,
            tool_name,
            source,
            payload,
        };

        self.registry
            .dispatch_any_with_terminal_outcome(invocation, terminal_outcome_reached)
            .await
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
