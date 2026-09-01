use crate::function_tool::FunctionCallError;
use crate::responses_metadata::TurnToolNamespacesInfo;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
#[cfg(test)]
use crate::session::turn_context::TurnContext;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
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
#[cfg(test)]
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ToolMode;
use codex_tools::DiscoverableTool;
use codex_tools::FreeformTool;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use sha2::Digest;
use sha2::Sha256;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

pub use crate::tools::context::ToolCallSource;

// This is a Codex model-context safety bound, not a Provider protocol limit.
const MAX_FLAT_ROUTE_CANONICAL_LABEL_BYTES: usize = 512;
const FLAT_ROUTE_CANONICAL_LABEL_DIGEST_HEX_CHARS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub tool_name: ToolName,
    pub call_id: String,
    pub payload: ToolPayload,
    pub encrypted_function_args: Option<Vec<String>>,
}

impl ToolCall {
    pub(crate) fn direct_source(&self) -> ToolCallSource {
        if is_plaintext_collaboration_tool(&self.tool_name)
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

fn is_plaintext_collaboration_tool(tool_name: &ToolName) -> bool {
    tool_name.namespace.as_deref() == Some("collaboration")
        && matches!(
            tool_name.name.as_str(),
            "spawn_agent" | "send_message" | "followup_task"
        )
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

/// One finalized tool plan: its advertised surfaces and matching executable runtimes.
pub struct ToolRouter {
    registry: ToolRegistry,
    model_visible_specs: Arc<[ToolSpec]>,
    tool_mode: ToolMode,
    code_mode_tool_names: BTreeMap<String, ToolName>,
    tool_namespaces_info: Option<TurnToolNamespacesInfo>,
    can_manage_children: bool,
    flat_tool_routes: FlatToolRoutes,
}

#[derive(Clone, Debug)]
enum WireToolRoute {
    Function(ToolName),
    Custom {
        tool_name: ToolName,
        input_key: String,
    },
}

impl WireToolRoute {
    fn tool_name(&self) -> &ToolName {
        match self {
            Self::Function(tool_name) | Self::Custom { tool_name, .. } => tool_name,
        }
    }
}

/// Compiled symbols for one finalized model-visible tool plan.
#[derive(Default)]
struct FlatToolRoutes {
    by_wire_name: BTreeMap<String, WireToolRoute>,
    by_canonical_name: BTreeMap<ToolName, String>,
    by_unique_short_name: BTreeMap<String, Option<String>>,
}

impl FlatToolRoutes {
    fn insert(&mut self, kind: &str, route: WireToolRoute) -> Result<String, String> {
        let mut wire_name = flat_wire_name(kind, route.tool_name());
        if self.by_wire_name.contains_key(&wire_name) {
            wire_name = digested_flat_wire_name(kind, route.tool_name());
        }
        if self.by_wire_name.contains_key(&wire_name) {
            return Err(wire_name);
        }
        let canonical_name = route.tool_name().clone().with_default_namespace();
        if self
            .by_canonical_name
            .insert(canonical_name, wire_name.clone())
            .is_some()
        {
            return Err(route.tool_name().to_string());
        }

        self.by_unique_short_name
            .entry(route.tool_name().name.clone())
            .and_modify(|entry| *entry = None)
            .or_insert_with(|| Some(wire_name.clone()));
        self.by_wire_name.insert(wire_name.clone(), route);
        Ok(wire_name)
    }

    fn resolve(&self, name: &str, namespace: &Option<String>) -> Option<&WireToolRoute> {
        if let Some(route) = self.by_wire_name.get(name) {
            return Some(route);
        }

        let canonical_name = ToolName::new(namespace.clone(), name).with_default_namespace();
        if !canonical_name.is_default_namespace() {
            return self
                .by_canonical_name
                .get(&canonical_name)
                .and_then(|wire_name| self.by_wire_name.get(wire_name));
        }

        // A Provider-default or unqualified echo is a lexical short name. The
        // compiler publishes that alias only when this finalized plan has one
        // matching canonical tool.
        self.by_canonical_name
            .get(&canonical_name)
            .or_else(|| self.by_unique_short_name.get(name).and_then(Option::as_ref))
            .and_then(|wire_name| self.by_wire_name.get(wire_name))
    }

    fn wire_name_for_function(&self, tool_name: &ToolName) -> Option<&str> {
        self.wire_name_for(tool_name, |route| {
            matches!(route, WireToolRoute::Function(_))
        })
    }

    fn wire_name_for_custom(&self, tool_name: &ToolName) -> Option<&str> {
        self.wire_name_for(tool_name, |route| {
            matches!(route, WireToolRoute::Custom { .. })
        })
    }

    fn wire_name_for(
        &self,
        tool_name: &ToolName,
        matches_kind: impl FnOnce(&WireToolRoute) -> bool,
    ) -> Option<&str> {
        let canonical_name = tool_name.clone().with_default_namespace();
        let wire_name = self.by_canonical_name.get(&canonical_name)?;
        self.by_wire_name
            .get(wire_name)
            .filter(|route| matches_kind(route))
            .map(|_| wire_name.as_str())
    }
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
        model_info: &ModelInfo,
        registry: ToolRegistry,
        hosted_specs: Vec<ToolSpec>,
        tool_search_handler_cache: &ToolSearchHandlerCache,
    ) -> Self {
        finalize_tool_router(
            turn_context,
            model_info,
            registry,
            hosted_specs,
            tool_search_handler_cache,
        )
        .expect("test tool registry should not contain duplicate tools")
    }

    pub(crate) fn from_parts(
        registry: ToolRegistry,
        model_visible_specs: Vec<ToolSpec>,
        tool_mode: ToolMode,
        code_mode_tool_names: BTreeMap<String, ToolName>,
        tool_namespaces_info: Option<TurnToolNamespacesInfo>,
        child_management_tools: &[ToolName],
    ) -> Self {
        let mut router = Self {
            registry,
            model_visible_specs: model_visible_specs.into(),
            tool_mode,
            code_mode_tool_names,
            tool_namespaces_info,
            can_manage_children: false,
            flat_tool_routes: FlatToolRoutes::default(),
        };
        router.can_manage_children = !child_management_tools.is_empty()
            && child_management_tools
                .iter()
                .all(|name| router.exposes_tool(name));
        router
    }

    pub(crate) fn from_parts_with_projection(
        registry: ToolRegistry,
        model_visible_specs: Vec<ToolSpec>,
        tool_mode: ToolMode,
        code_mode_tool_names: BTreeMap<String, ToolName>,
        tool_namespaces_info: Option<TurnToolNamespacesInfo>,
        child_management_tools: &[ToolName],
        project_as_flat_functions: bool,
    ) -> Result<Self, String> {
        if !project_as_flat_functions {
            return Ok(Self::from_parts(
                registry,
                model_visible_specs,
                tool_mode,
                code_mode_tool_names,
                tool_namespaces_info,
                child_management_tools,
            ));
        }
        let (model_visible_specs, flat_tool_routes) =
            project_flat_function_tools(model_visible_specs)?;
        let mut router = Self {
            registry,
            model_visible_specs: model_visible_specs.into(),
            tool_mode,
            code_mode_tool_names,
            tool_namespaces_info,
            can_manage_children: false,
            flat_tool_routes,
        };
        router.can_manage_children = !child_management_tools.is_empty()
            && child_management_tools
                .iter()
                .all(|name| router.exposes_tool(name));
        Ok(router)
    }

    pub(crate) fn model_visible_specs(&self) -> Arc<[ToolSpec]> {
        Arc::clone(&self.model_visible_specs)
    }

    pub(crate) fn tool_mode(&self) -> ToolMode {
        self.tool_mode
    }

    /// Code Mode still needs its dispatcher when the nested tool set is empty.
    pub(crate) fn requires_code_mode_worker(&self) -> bool {
        matches!(self.tool_mode, ToolMode::CodeMode | ToolMode::CodeModeOnly)
    }

    /// The normalized nested identities chosen after exclusions and collisions.
    // Consumed by the follow-up cell-origin migration.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn code_mode_tool_names(&self) -> &BTreeMap<String, ToolName> {
        &self.code_mode_tool_names
    }

    /// Optional request inventory for this exact plan, without publishing it to turn state.
    pub(crate) fn tool_namespaces_info(&self) -> Option<&TurnToolNamespacesInfo> {
        self.tool_namespaces_info.as_ref()
    }

    /// Whether the model can both start and interact with a terminal process.
    // Consumed by the follow-up live tool-plan selection.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn has_terminal_controls(&self) -> bool {
        self.exposes_tool(&ToolName::plain("exec_command"))
            && self.exposes_tool(&ToolName::plain("write_stdin"))
    }

    /// Whether the configured collaboration backend's child-management tools remain exposed.
    // Consumed by the follow-up live tool-plan selection.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn can_manage_children(&self) -> bool {
        self.can_manage_children
    }

    // Answers if the tool plan lets the model invoke the tool directly, through code mode, or deferred tool search.
    fn exposes_tool(&self, name: &ToolName) -> bool {
        let name = name.clone().with_default_namespace();
        if self
            .code_mode_tool_names
            .values()
            .any(|nested| nested.clone().with_default_namespace() == name)
            || self.model_visible_specs.iter().any(|spec| match spec {
                ToolSpec::Function(_) | ToolSpec::Freeform(_) => {
                    name.is_default_namespace() && spec.name() == name.name
                }
                ToolSpec::Namespace(namespace) => {
                    name.namespace.as_deref() == Some(namespace.name.as_str())
                        && namespace.tools.iter().any(|tool| match tool {
                            ResponsesApiNamespaceTool::Function(tool) => tool.name == name.name,
                            ResponsesApiNamespaceTool::Custom(tool) => tool.name == name.name,
                        })
                }
                ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } | ToolSpec::XSearch => {
                    false
                }
            })
        {
            return true;
        }
        self.model_visible_specs
            .iter()
            .any(|spec| matches!(spec, ToolSpec::ToolSearch { .. }))
            && self.registry.entries().any(|tool| {
                tool.exposure.is_deferred()
                    && tool.runtime.tool_name().with_default_namespace() == name
                    && (tool.runtime.immutable_spec().is_some()
                        || tool.runtime.search_info().is_some())
            })
    }

    pub(crate) fn project_model_input(&self, mut input: Vec<ResponseItem>) -> Vec<ResponseItem> {
        if self.flat_tool_routes.by_wire_name.is_empty() {
            return input;
        }
        for item in &mut input {
            match item {
                ResponseItem::FunctionCall {
                    name,
                    namespace,
                    encrypted_function_args,
                    ..
                } => {
                    let tool_name = ToolName::new(namespace.take(), name.clone());
                    if let Some(wire_name) =
                        self.flat_tool_routes.wire_name_for_function(&tool_name)
                    {
                        *name = wire_name.to_string();
                        *encrypted_function_args = None;
                    } else {
                        *namespace = tool_name.namespace;
                    }
                }
                ResponseItem::CustomToolCall {
                    id,
                    call_id,
                    name,
                    namespace,
                    input,
                    internal_chat_message_metadata_passthrough,
                    ..
                } => {
                    let tool_name = ToolName::new(namespace.clone(), name.clone());
                    if let Some(wire_name) = self.flat_tool_routes.wire_name_for_custom(&tool_name)
                    {
                        *item = ResponseItem::FunctionCall {
                            id: id.clone(),
                            name: wire_name.to_string(),
                            namespace: None,
                            arguments: serde_json::json!({
                                (custom_input_key(&tool_name.name)): input,
                            })
                            .to_string(),
                            encrypted_function_args: None,
                            call_id: call_id.clone(),
                            internal_chat_message_metadata_passthrough:
                                internal_chat_message_metadata_passthrough.clone(),
                        };
                    }
                }
                _ => {}
            }
        }
        input
    }

    pub(crate) fn exposes_x_search(&self) -> bool {
        self.model_visible_specs
            .iter()
            .any(|spec| matches!(spec, ToolSpec::XSearch))
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

    pub fn tool_supports_parallel(&self, call: &ToolCall) -> bool {
        self.registry
            .supports_parallel_tool_calls(&call.tool_name)
            .unwrap_or(false)
    }

    pub(crate) fn tool_runtime(&self, call: &ToolCall) -> Option<Arc<dyn CoreToolRuntime>> {
        self.registry.tool(&call.tool_name)
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

    fn resolve_wire_route(&self, name: &str, namespace: &Option<String>) -> Option<&WireToolRoute> {
        self.flat_tool_routes.resolve(name, namespace)
    }

    pub(crate) fn restore_tool_call(
        &self,
        item: &mut ResponseItem,
    ) -> Result<(), FunctionCallError> {
        let ResponseItem::FunctionCall {
            id,
            name,
            namespace,
            arguments,
            encrypted_function_args,
            call_id,
            internal_chat_message_metadata_passthrough,
            ..
        } = item
        else {
            return Ok(());
        };
        let Some(route) = self.resolve_wire_route(name, namespace) else {
            return Ok(());
        };
        match route {
            WireToolRoute::Function(tool_name) => {
                *name = tool_name.name.clone();
                *namespace = tool_name.namespace.clone();
                if is_plaintext_collaboration_tool(tool_name) {
                    *encrypted_function_args = Some(Vec::new());
                }
            }
            WireToolRoute::Custom {
                tool_name,
                input_key,
            } => {
                let restored = ResponseItem::CustomToolCall {
                    id: id.clone(),
                    status: None,
                    call_id: call_id.clone(),
                    name: tool_name.name.clone(),
                    namespace: tool_name.namespace.clone(),
                    input: decode_custom_input(name, arguments, input_key)?,
                    internal_chat_message_metadata_passthrough:
                        internal_chat_message_metadata_passthrough.clone(),
                };
                *item = restored;
            }
        }
        Ok(())
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

fn project_flat_function_tools(
    specs: Vec<ToolSpec>,
) -> Result<(Vec<ToolSpec>, FlatToolRoutes), String> {
    let mut declarations = Vec::new();
    let mut routes = FlatToolRoutes::default();
    for spec in specs {
        match spec {
            ToolSpec::Function(tool) => {
                let tool_name = ToolName::plain(tool.name.clone());
                let wire_name =
                    routes.insert("function", WireToolRoute::Function(tool_name.clone()))?;
                declarations.push(ToolSpec::Function(function_declaration(
                    wire_name, &tool_name, tool,
                )));
            }
            ToolSpec::Freeform(tool) => {
                let tool_name = ToolName::plain(tool.name.clone());
                let input_key = custom_input_key(&tool_name.name).to_string();
                let wire_name = routes.insert(
                    "custom",
                    WireToolRoute::Custom {
                        tool_name: tool_name.clone(),
                        input_key: input_key.clone(),
                    },
                )?;
                let tool = custom_function_declaration(wire_name, &tool_name, tool, &input_key);
                declarations.push(ToolSpec::Function(tool));
            }
            ToolSpec::Namespace(namespace) => {
                for tool in namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(tool) => {
                            let tool_name =
                                ToolName::namespaced(namespace.name.clone(), tool.name.clone());
                            let wire_name = routes
                                .insert("function", WireToolRoute::Function(tool_name.clone()))?;
                            declarations.push(ToolSpec::Function(function_declaration(
                                wire_name, &tool_name, tool,
                            )));
                        }
                        ResponsesApiNamespaceTool::Custom(tool) => {
                            let tool_name =
                                ToolName::namespaced(namespace.name.clone(), tool.name.clone());
                            let input_key = custom_input_key(&tool_name.name).to_string();
                            let wire_name = routes.insert(
                                "custom",
                                WireToolRoute::Custom {
                                    tool_name: tool_name.clone(),
                                    input_key: input_key.clone(),
                                },
                            )?;
                            let tool = custom_function_declaration(
                                wire_name, &tool_name, tool, &input_key,
                            );
                            declarations.push(ToolSpec::Function(tool));
                        }
                    }
                }
            }
            hosted => declarations.push(hosted),
        }
    }
    Ok((declarations, routes))
}

fn function_declaration(
    wire_name: String,
    tool_name: &ToolName,
    mut tool: ResponsesApiTool,
) -> ResponsesApiTool {
    tool.name = wire_name;
    tool.description = flat_route_description(tool_name, &tool.description);
    tool.defer_loading = None;
    tool
}

fn custom_function_declaration(
    wire_name: String,
    tool_name: &ToolName,
    tool: FreeformTool,
    input_key: &str,
) -> ResponsesApiTool {
    let parameters = JsonSchema::object(
        BTreeMap::from([(
            input_key.to_string(),
            JsonSchema::string(Some(
                "Freeform input passed unchanged to Codex.".to_string(),
            )),
        )]),
        Some(vec![input_key.to_string()]),
        Some(false.into()),
    );
    let description = flat_route_description(
        tool_name,
        &format!("{}\n\n{}", tool.description, tool.format.definition),
    );
    ResponsesApiTool {
        name: wire_name,
        description,
        strict: true,
        defer_loading: None,
        parameters,
        output_schema: None,
    }
}

fn flat_route_description(tool_name: &ToolName, description: &str) -> String {
    let canonical_name = if tool_name.is_default_namespace() {
        tool_name.name.clone()
    } else {
        format!(
            "{}.{}",
            tool_name.namespace.as_deref().unwrap_or_default(),
            tool_name.name
        )
    };
    let digest = format!("{:x}", Sha256::digest(canonical_name.as_bytes()));
    let digest = &digest[..FLAT_ROUTE_CANONICAL_LABEL_DIGEST_HEX_CHARS];
    let mut sanitized_name = canonical_name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '_' | '-' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized_name != canonical_name
        || sanitized_name.len() > MAX_FLAT_ROUTE_CANONICAL_LABEL_BYTES
    {
        let separator = "__";
        let prefix_budget = MAX_FLAT_ROUTE_CANONICAL_LABEL_BYTES
            .saturating_sub(separator.len())
            .saturating_sub(FLAT_ROUTE_CANONICAL_LABEL_DIGEST_HEX_CHARS);
        let prefix_end = sanitized_name
            .char_indices()
            .take_while(|(index, character)| index + character.len_utf8() <= prefix_budget)
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        sanitized_name.truncate(prefix_end);
        sanitized_name.push_str(separator);
        sanitized_name.push_str(digest);
    }
    format!(
        "This flat Provider function directly invokes the canonical `{sanitized_name}` tool. Call this function itself. Do not invoke the canonical tool through a shell, code-mode wrapper, or another tool; any such invocation guidance in the retained description does not apply to this flat interface.\n\n{description}"
    )
}

fn flat_wire_name(kind: &str, tool_name: &ToolName) -> String {
    // This is a Codex model-context safety bound, not a Provider protocol limit.
    const MAX_MODEL_CONTEXT_FLAT_WIRE_NAME_BYTES: usize = 1_024;
    const WIRE_NAME_PREFIX: &str = "local__";

    let original_semantic_name = codex_tools::code_mode_name_for_tool_name(tool_name);
    let mut semantic_name = original_semantic_name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    if !semantic_name.is_empty()
        && semantic_name == original_semantic_name
        && WIRE_NAME_PREFIX.len() + semantic_name.len() <= MAX_MODEL_CONTEXT_FLAT_WIRE_NAME_BYTES
    {
        return format!("{WIRE_NAME_PREFIX}{semantic_name}");
    }
    digested_flat_wire_name(kind, tool_name)
}

fn digested_flat_wire_name(kind: &str, tool_name: &ToolName) -> String {
    const MAX_MODEL_CONTEXT_FLAT_WIRE_NAME_BYTES: usize = 1_024;
    const WIRE_NAME_PREFIX: &str = "local__";
    const WIRE_NAME_SEPARATOR: &str = "__";
    const WIRE_ROUTE_DIGEST_HEX_CHARS: usize = 32;

    let namespace = if tool_name.is_default_namespace() {
        ""
    } else {
        tool_name.namespace.as_deref().unwrap_or_default()
    };
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{kind}\0{namespace}\0{}", tool_name.name).as_bytes())
    );
    let semantic_name = codex_tools::code_mode_name_for_tool_name(tool_name);
    let mut semantic_name = semantic_name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    let digest = &digest[..WIRE_ROUTE_DIGEST_HEX_CHARS];
    if !semantic_name.is_empty() {
        let semantic_budget = MAX_MODEL_CONTEXT_FLAT_WIRE_NAME_BYTES
            .saturating_sub(WIRE_NAME_PREFIX.len())
            .saturating_sub(WIRE_NAME_SEPARATOR.len())
            .saturating_sub(WIRE_ROUTE_DIGEST_HEX_CHARS);
        semantic_name.truncate(semantic_budget);
        return format!("{WIRE_NAME_PREFIX}{semantic_name}{WIRE_NAME_SEPARATOR}{digest}");
    }
    format!("{WIRE_NAME_PREFIX}{digest}")
}

fn custom_input_key(tool_name: &str) -> &'static str {
    match tool_name {
        "apply_patch" => "patch",
        "exec" => "source",
        _ => "input",
    }
}

fn decode_custom_input(
    wire_name: &str,
    arguments: &str,
    input_key: &str,
) -> Result<String, FunctionCallError> {
    let value: serde_json::Value = serde_json::from_str(arguments).map_err(|error| {
        FunctionCallError::RespondToModel(format!("invalid arguments for `{wire_name}`: {error}"))
    })?;
    let input = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get(input_key))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(format!(
                "invalid arguments for `{wire_name}`: expected one string field `{input_key}`"
            ))
        })?;
    Ok(input.to_string())
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
