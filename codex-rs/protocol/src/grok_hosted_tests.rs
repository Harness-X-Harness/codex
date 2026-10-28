use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn completed_x_names_require_provider_identity_and_terminal_payload() {
    let base = json!({"type":"custom_tool_call", "id":"x-id", "call_id":"x-call",
        "name":"x_keyword_search", "status":"completed", "input":"query"});
    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        let mut value = base.clone();
        value["name"] = json!(name);
        assert!(is_completed_x_search(
            &serde_json::from_value(value).unwrap()
        ));
    }
    for (key, value) in [
        ("id", json!(null)),
        ("id", json!("")),
        ("call_id", json!("")),
        ("name", json!("x_unknown")),
        ("name", json!("apply_patch")),
        ("namespace", json!("functions")),
        ("status", json!(null)),
        ("status", json!("in_progress")),
        ("status", json!("failed")),
    ] {
        let mut item = base.clone();
        item[key] = value;
        assert!(
            !is_completed_x_search(&serde_json::from_value(item).unwrap()),
            "{key}"
        );
    }
}

#[test]
fn web_completion_requires_a_lossless_supported_action() {
    for action in [
        json!({"type":"search", "query":"query"}),
        json!({"type":"search", "queries":["a","b"]}),
        json!({"type":"open_page", "url":"https://example.com"}),
        json!({"type":"find_in_page", "url":"https://example.com", "pattern":"term"}),
        json!({"type":"search"}),
        json!({"type":"search", "queries":[]}),
        json!({"type":"open_page", "url":""}),
        json!({"type":"open_page"}),
        json!({"type":"find_in_page", "url":"https://example.com"}),
        json!({"type":"find_in_page", "pattern":""}),
    ] {
        let item = serde_json::from_value(
            json!({"type":"web_search_call", "id":"web-id", "status":"completed", "action":action}),
        )
        .unwrap();
        assert!(is_completed_web_search(&item));
        assert_eq!(project_search_replay(&item).unwrap()["action"], action);
    }
    for action in [json!(null), json!({"type":"unknown"})] {
        let item = serde_json::from_value(
            json!({"type":"web_search_call", "id":"web-id", "status":"completed", "action":action}),
        )
        .unwrap();
        assert!(!is_completed_web_search(&item));
    }
}

#[test]
fn hosted_replay_bound_is_exact_non_truncating_and_separate_from_identity() {
    let max = codex_utils_string::approx_bytes_for_tokens(MAX_HOSTED_REPLAY_TOKENS);
    for unit in ["a", "雪", "\n", "\""] {
        let mut raw = json!({"type":"custom_tool_call", "id":"x", "call_id":"call", "name":"x_keyword_search",
            "status":"completed", "input":""});
        let empty: ResponseItem = serde_json::from_value(raw.clone()).unwrap();
        let overhead = serde_json::to_vec(&project_search_replay(&empty).unwrap())
            .unwrap()
            .len();
        let unit_bytes = serde_json::to_vec(unit).unwrap().len() - 2;
        let repetitions = (max - overhead) / unit_bytes;
        raw["input"] = json!(unit.repeat(repetitions));
        let at: ResponseItem = serde_json::from_value(raw.clone()).unwrap();
        let projected = project_search_replay(&at).unwrap();
        assert_eq!(projected["input"], raw["input"]);
        assert!(serde_json::to_vec(&projected).unwrap().len() <= max);
        raw["input"] = json!(unit.repeat(repetitions + 1));
        let above: ResponseItem = serde_json::from_value(raw).unwrap();
        assert!(is_completed_x_search(&above));
        assert!(
            project_search_replay(&above)
                .unwrap_err()
                .contains("context limit")
        );
    }
}

#[test]
fn non_replayed_metadata_does_not_consume_hosted_context_budget() {
    let item: ResponseItem = serde_json::from_value(json!({
        "type":"custom_tool_call", "id":"x", "call_id":"call", "name":"x_keyword_search",
        "status":"completed", "input":"query",
        "internal_chat_message_metadata_passthrough":{"turn_id":"m".repeat(50_000)}
    }))
    .unwrap();
    assert_eq!(
        project_search_replay(&item).unwrap(),
        json!({
            "type":"custom_tool_call", "id":"x", "call_id":"call", "name":"x_keyword_search", "input":"query"
        })
    );
}

#[test]
fn web_replay_query_bound_uses_exact_wire_fields_without_metadata() {
    let max = codex_utils_string::approx_bytes_for_tokens(MAX_HOSTED_REPLAY_TOKENS);
    let mut raw = json!({
        "type":"web_search_call", "id":"web-id", "status":"completed",
        "action":{"type":"search", "query":"q"},
        "internal_chat_message_metadata_passthrough":{"turn_id":"m".repeat(max + 1)}
    });
    let small: ResponseItem = serde_json::from_value(raw.clone()).unwrap();
    let overhead = serde_json::to_vec(&project_search_replay(&small).unwrap())
        .unwrap()
        .len()
        - 1;
    for wire_bytes in [max - 1, max] {
        let query = "q".repeat(wire_bytes - overhead);
        raw["action"]["query"] = json!(query);
        let item: ResponseItem = serde_json::from_value(raw.clone()).unwrap();
        assert!(serde_json::to_vec(&item).unwrap().len() > max);
        let projected = project_search_replay(&item).unwrap();
        assert_eq!(
            projected,
            json!({
                "type":"web_search_call", "id":"web-id",
                "action":{"type":"search", "query":query}
            })
        );
        assert_eq!(serde_json::to_vec(&projected).unwrap().len(), wire_bytes);
    }
    raw["action"]["query"] = json!("q".repeat(max - overhead + 1));
    let above: ResponseItem = serde_json::from_value(raw).unwrap();
    assert!(is_completed_web_search(&above));
    assert_eq!(
        project_search_replay(&above).unwrap_err(),
        "Grok hosted replay exceeds the 10000 estimated-token context limit"
    );
}
