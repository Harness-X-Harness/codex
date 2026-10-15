use codex_app_server_protocol::ThreadItem;
use serde_json::Value;
use serde_json::json;

pub(super) fn tool_marker(item: &ThreadItem, turn_id: &str) -> Option<Value> {
    match item {
        ThreadItem::CommandExecution { id, status, .. } => Some(json!({
            "id": id, "turnId": turn_id, "type": "commandExecution",
            "name": "commandExecution", "status": status
        })),
        ThreadItem::FileChange { id, status, .. } => Some(json!({
            "id": id, "turnId": turn_id, "type": "fileChange",
            "name": "fileChange", "status": status
        })),
        ThreadItem::ImageGeneration(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "imageGeneration",
            "name": "imageGeneration", "status": item.status
        })),
        ThreadItem::McpToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "mcpToolCall",
            "name": tool, "status": status
        })),
        ThreadItem::DynamicToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "dynamicToolCall",
            "name": tool, "status": status
        })),
        ThreadItem::CollabAgentToolCall {
            id, tool, status, ..
        } => Some(json!({
            "id": id, "turnId": turn_id, "type": "collabAgentToolCall",
            "name": tool, "status": status
        })),
        ThreadItem::Sleep(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "sleep",
            "name": "sleep", "status": null
        })),
        ThreadItem::WebSearch(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "webSearch",
            "name": "webSearch", "status": null
        })),
        ThreadItem::XSearch(item) => Some(json!({
            "id": item.id, "turnId": turn_id, "type": "xSearch",
            "name": item.name, "status": null
        })),
        ThreadItem::UserMessage { .. }
        | ThreadItem::FunctionCallOutput { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::SubAgentActivity { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => None,
    }
}

#[cfg(test)]
#[path = "dynamic_tools_markers_tests.rs"]
mod tests;
