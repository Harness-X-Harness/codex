use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn retired_hosted_history_is_rejected_without_rewriting_payloads() {
    for record in [
        json!({"type":"event_msg", "payload":{"type":"item_completed", "item":{"type":"XSearch", "id":"x"}}}),
        json!({"type":"event_msg", "payload":{"type":"item_started", "item":{"type":"XSearch", "id":"x"}}}),
        json!({"type":"xSearch", "id":"x"}),
    ] {
        let original = record.clone();
        let error = validate_supported_history(&record).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("retired Hosted X Search"));
        assert_eq!(record, original);
    }
}

#[test]
fn hosted_history_guard_preserves_web_local_custom_and_user_payloads() {
    for record in [
        json!({"type":"response_item", "payload":{"type":"web_search_call", "id":"web", "status":"completed", "action":{"type":"search", "query":"query"}}}),
        json!({"type":"response_item", "payload":{"type":"custom_tool_call", "name":"x_keyword_search", "call_id":"local", "status":"completed", "input":"provider-neutral custom call"}}),
        json!({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"xSearch custom_tool_call"}]}}),
        json!({"type":"event_msg", "payload":{"type":"agent_message", "message":"xSearch"}}),
    ] {
        assert!(validate_supported_history(&record).is_ok());
    }
}
