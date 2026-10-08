//! Outgoing copies use stable names even for tools no longer in the active plan.

use crate::FlatToolRoutes;
use crate::ToolName;
use crate::WireToolRoute;
use crate::flat_projection::custom_route;
use crate::flat_projection::normalize_name;
use codex_protocol::models::ResponseItem;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

impl FlatToolRoutes {
    pub fn project_history(&self, history: &[ResponseItem]) -> Result<Vec<ResponseItem>, String> {
        let local_custom_outputs = history
            .iter()
            .filter_map(|item| match item {
                ResponseItem::CustomToolCallOutput { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        // Canonical local custom history may carry completed status. A matching
        // local output takes precedence over the provider-hosted shape, including
        // local tools whose names happen to match an X-search name.
        let is_hosted = |item: &ResponseItem| {
            codex_protocol::grok_hosted::is_completed_search(item)
                && !matches!(item, ResponseItem::CustomToolCall { call_id, .. }
                    if local_custom_outputs.contains(call_id.as_str()))
        };
        let mut routes = self.clone();
        let mut calls = BTreeMap::new();
        for item in history {
            if is_hosted(item) {
                codex_protocol::grok_hosted::project_search_replay(item)?;
                continue;
            }
            let (call_id, route) = match item {
                ResponseItem::CustomToolCall {
                    status: Some(_),
                    call_id,
                    ..
                } if !local_custom_outputs.contains(call_id.as_str()) => {
                    return Err("unsupported hosted custom history".to_string());
                }
                ResponseItem::FunctionCall {
                    call_id,
                    name,
                    namespace,
                    ..
                } => (
                    call_id,
                    WireToolRoute::Function(normalize_name(ToolName::new(namespace.clone(), name))),
                ),
                ResponseItem::CustomToolCall {
                    call_id,
                    name,
                    namespace,
                    ..
                } => (
                    call_id,
                    custom_route(ToolName::new(namespace.clone(), name)),
                ),
                _ => continue,
            };
            let wire_name = routes.remember(route.clone())?;
            if calls.insert(call_id.clone(), (wire_name, route)).is_some() {
                return Err(format!("duplicate history tool call ID: {call_id}"));
            }
        }
        history.iter().cloned().map(|item| match item {
            item if is_hosted(&item) => Ok(item),
            ResponseItem::FunctionCall {
                id, name, namespace, arguments, encrypted_function_args, call_id,
                internal_chat_message_metadata_passthrough,
            } => {
                if let Some(envelope) = encrypted_function_args
                    && !(envelope.is_empty()
                        && namespace.as_deref() == Some("collaboration")
                        && matches!(name.as_str(), "spawn_agent" | "send_message" | "followup_task"))
                {
                    return Err("flat local tools cannot replay encrypted function arguments".to_string());
                }
                // The recognized empty collaboration envelope is a local logging marker,
                // not encrypted provider data. Remove it only from this outgoing copy.
                Ok(ResponseItem::FunctionCall {
                    id, name: routes.remember(WireToolRoute::Function(normalize_name(ToolName::new(namespace, name))))?,
                    namespace: None, arguments, encrypted_function_args: None, call_id,
                    internal_chat_message_metadata_passthrough,
                })
            },
            ResponseItem::CustomToolCall {
                id, call_id, name, namespace, input, internal_chat_message_metadata_passthrough, ..
            } => {
                let route = custom_route(ToolName::new(namespace, name));
                let WireToolRoute::Custom { input_key, .. } = &route else { unreachable!() };
                let arguments = serde_json::to_string(&BTreeMap::from([(input_key, input)]))
                    .map_err(|error| error.to_string())?;
                Ok(ResponseItem::FunctionCall {
                    id, name: routes.remember(route)?, namespace: None, arguments,
                    encrypted_function_args: None, call_id, internal_chat_message_metadata_passthrough,
                })
            }
            ResponseItem::FunctionCallOutput {
                id, call_id, name, namespace, output, internal_chat_message_metadata_passthrough,
            } => {
                let paired = call_id.as_ref().and_then(|id| calls.get(id));
                if matches!(paired, Some((_, WireToolRoute::Custom { .. }))) {
                    return Err("function output paired with a custom call".to_string());
                }
                let name = match (name, paired) {
                    (Some(name), Some((wire_name, WireToolRoute::Function(tool_name)))) => {
                        let supplied_namespace = namespace.as_ref().map(|_| {
                            normalize_name(ToolName::new(namespace.clone(), &name)).namespace
                        });
                        if name != tool_name.name
                            || supplied_namespace.is_some_and(|namespace| namespace != tool_name.namespace)
                        {
                            return Err("function output identity differs from its call".to_string());
                        }
                        Some(wire_name.clone())
                    }
                    (Some(name), _) => Some(routes.remember(WireToolRoute::Function(
                        normalize_name(ToolName::new(namespace, name)),
                    ))?),
                    (None, _) => None,
                };
                Ok(ResponseItem::FunctionCallOutput {
                    id, call_id, name, namespace: None, output, internal_chat_message_metadata_passthrough,
                })
            }
            ResponseItem::CustomToolCallOutput {
                id, call_id, name, output, internal_chat_message_metadata_passthrough,
            } => {
                let Some((wire_name, WireToolRoute::Custom { tool_name, .. })) = calls.get(&call_id) else {
                    return Err(format!("custom output has no paired custom call: {call_id}"));
                };
                if let Some(name) = &name
                    && name != &tool_name.name
                {
                    return Err("custom output identity differs from its call".to_string());
                }
                Ok(ResponseItem::FunctionCallOutput {
                    id, call_id: Some(call_id), name: name.map(|_| wire_name.clone()), namespace: None,
                    output, internal_chat_message_metadata_passthrough,
                })
            }
            ResponseItem::AdditionalTools { .. } | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::ToolSearchOutput { .. } | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. } | ResponseItem::LocalShellCall { .. } => {
                Err("flat local tool history contains an unsupported hosted, search, or legacy tool".to_string())
            }
            item => Ok(item),
        }).collect()
    }
}
