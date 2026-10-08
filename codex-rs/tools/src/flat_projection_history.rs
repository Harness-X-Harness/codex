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
        let mut pending_calls: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut local_custom_calls = BTreeSet::new();
        for (index, item) in history.iter().enumerate() {
            match item {
                ResponseItem::FunctionCall { call_id, .. }
                | ResponseItem::CustomToolCall { call_id, .. } => {
                    let candidates = pending_calls.entry(call_id.as_str()).or_default();
                    // Retain a paired occurrence for duplicate outputs only until
                    // another call with this response-local ID arrives.
                    if candidates
                        .first()
                        .is_some_and(|index| local_custom_calls.contains(index))
                    {
                        candidates.clear();
                    }
                    candidates.push(index);
                }
                ResponseItem::CustomToolCallOutput { call_id, name, .. } => {
                    let Some(candidates) = pending_calls.get_mut(call_id.as_str()) else {
                        return Err(format!(
                            "custom output has no paired custom call: {call_id}"
                        ));
                    };
                    // Local execution outputs can follow intervening hosted items
                    // from the same response. Prefer the uniquely local-shaped call.
                    let mut local = candidates.iter().copied().filter(|index| {
                        !codex_protocol::grok_hosted::is_completed_search(&history[*index])
                    });
                    let call_index = match (local.next(), local.next()) {
                        (Some(index), None) => index,
                        (None, None) => {
                            // Historical local X calls can have the hosted shape.
                            // An explicit output name is additional identity evidence.
                            let mut matching = candidates.iter().copied().filter(|index| {
                                matches!(&history[*index], ResponseItem::CustomToolCall {
                                    name: call_name, ..
                                } if name.as_ref().is_none_or(|name| name == call_name))
                            });
                            let Some(index) = matching.next() else {
                                return Err(
                                    "custom output identity differs from its call".to_string()
                                );
                            };
                            if matching.next().is_some() {
                                return Err(format!("ambiguous custom output history: {call_id}"));
                            }
                            index
                        }
                        _ => return Err(format!("ambiguous custom output history: {call_id}")),
                    };
                    if !matches!(history[call_index], ResponseItem::CustomToolCall { .. }) {
                        return Err(format!(
                            "custom output has no paired custom call: {call_id}"
                        ));
                    }
                    local_custom_calls.insert(call_index);
                    candidates.clear();
                    candidates.push(call_index);
                }
                _ => {}
            }
        }
        // Canonical local custom history may carry completed status, including
        // X-search names. Only its paired occurrence takes precedence over the
        // hosted shape; an older output cannot reclassify a later hosted item.
        let is_hosted = |index: usize, item: &ResponseItem| {
            codex_protocol::grok_hosted::is_completed_search(item)
                && !local_custom_calls.contains(&index)
        };
        let mut routes = self.clone();
        let mut calls = BTreeMap::new();
        for (index, item) in history.iter().enumerate() {
            if is_hosted(index, item) {
                codex_protocol::grok_hosted::project_search_replay(item)?;
                continue;
            }
            let (call_id, route) = match item {
                ResponseItem::CustomToolCall {
                    status: Some(_), ..
                } if !local_custom_calls.contains(&index) => {
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
        history.iter().cloned().enumerate().map(|(index, item)| match item {
            item if is_hosted(index, &item) => Ok(item),
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
