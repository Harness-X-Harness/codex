use codex_app_server_protocol::ThreadItemPayload;
use serde_json::Value;
use serde_json::json;

pub(super) fn tool_marker(item: &ThreadItemPayload, turn_id: &str) -> Option<Value> {
    match item {
        ThreadItemPayload::CommandExecution { id, status, .. } => Some(json!({
            "id": id, "turnId": turn_id, "type": "commandExecution",
            "name": "commandExecution", "status": status
        })),
        ThreadItemPayload::FileChange { id, status, .. } => Some(json!({
            "id": id, "turnId": turn_id, "type": "fileChange",
            "name": "fileChange", "status": status
        })),
        ThreadItemPayload::ImageGeneration(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "imageGeneration",
            "name": "imageGeneration", "status": item.status
        })),
        ThreadItemPayload::McpToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "mcpToolCall",
            "name": tool, "status": status
        })),
        ThreadItemPayload::DynamicToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "dynamicToolCall",
            "name": tool, "status": status
        })),
        ThreadItemPayload::CollabAgentToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "collabAgentToolCall",
            "name": tool, "status": status
        })),
        ThreadItemPayload::Sleep(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "sleep",
            "name": "sleep", "status": null
        })),
        ThreadItemPayload::WebSearch(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "webSearch",
            "name": "webSearch", "status": null
        })),
        ThreadItemPayload::UserMessage { .. }
        | ThreadItemPayload::FunctionCallOutput { .. }
        | ThreadItemPayload::HookPrompt { .. }
        | ThreadItemPayload::AgentMessage { .. }
        | ThreadItemPayload::Plan { .. }
        | ThreadItemPayload::Reasoning { .. }
        | ThreadItemPayload::SubAgentActivity { .. }
        | ThreadItemPayload::ImageView { .. }
        | ThreadItemPayload::EnteredReviewMode { .. }
        | ThreadItemPayload::ExitedReviewMode { .. }
        | ThreadItemPayload::ContextCompaction { .. } => None,
    }
}

#[cfg(test)]
#[path = "dynamic_tools_markers_tests.rs"]
mod tests;
