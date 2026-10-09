//! Source-shaped fixtures from grok-build 2bdd1d6 / async-openai 95b52eb.
//! They establish deterministic support, not fresh backend observations.
use super::*;
use codex_protocol::SearchActivityKind;
use codex_protocol::SearchActivityState;
use pretty_assertions::assert_eq;

fn x_item(name: &str, status: &str) -> Value {
    json!({"type":"custom_tool_call","id":"x-1","call_id":"shared-call", "name":name,
        "status":status,"input":"{ \"query\": \"exact 日本語\" }\n"})
}

fn input_done() -> Value {
    json!({"type":"response.custom_tool_call_input.done","output_index":1,"item_id":"x-1",
        "input":x_item("x_keyword_search", "completed")["input"]})
}

fn activities(
    events: &StreamEvents,
) -> Vec<(u64, String, SearchActivityKind, SearchActivityState)> {
    events
        .iter()
        .filter_map(|event| match event {
            Ok(ResponseEvent::SearchActivity {
                output_index,
                item_id,
                kind,
                state,
            }) => Some((*output_index, item_id.clone(), *kind, *state)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn grok_x_activity_requires_exact_shape_and_input_done_not_delta() -> Result<()> {
    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        let start = item_frame(
            "response.output_item.added",
            /*index*/ 1,
            x_item(name, "in_progress"),
        );
        let mut events = vec![
            added(/*index*/ 0),
            start,
            json!({"type":"response.custom_tool_call_input.delta","output_index":1,"item_id":"x-1","delta":"partial"}),
        ];
        assert!(activities(&run_web_events(events.clone(), ApiDialect::Grok).await?).is_empty());
        events.extend([
            input_done(),
            input_done(),
            item_frame(
                "response.output_item.done",
                /*index*/ 1,
                x_item(name, "completed"),
            ),
            done(/*index*/ 0),
            completed(),
        ]);
        for result in [
            run_events(events.clone(), ApiDialect::Grok).await?,
            run_web_events(events.clone(), ApiDialect::OpenAi).await?,
        ] {
            assert!(activities(&result).is_empty());
        }
        let result = run_web_events(events, ApiDialect::Grok).await?;
        assert!(matches!(
            result.last(),
            Some(Ok(ResponseEvent::Completed { .. }))
        ));
        assert_eq!(
            activities(&result),
            vec![
                (
                    1,
                    "x-1".into(),
                    SearchActivityKind::X,
                    SearchActivityState::Running
                ),
                (
                    1,
                    "x-1".into(),
                    SearchActivityKind::X,
                    SearchActivityState::Completed
                )
            ]
        );
    }
    Ok(())
}

#[tokio::test]
async fn grok_x_activity_does_not_steal_local_or_unknown_custom_calls() -> Result<()> {
    for mut item in [
        x_item("x_unknown", "in_progress"),
        x_item("apply_patch", "in_progress"),
        x_item("x_keyword_search", "in_progress"),
    ] {
        if item["name"] == "x_keyword_search" {
            item["namespace"] = json!("functions");
        }
        let mut finished = item.clone();
        finished["status"] = json!("completed");
        let result = run_web_events(
            vec![
                added(/*index*/ 0),
                item_frame("response.output_item.added", /*index*/ 1, item),
                input_done(),
                item_frame("response.output_item.done", /*index*/ 1, finished),
                done(/*index*/ 0),
                completed(),
            ],
            ApiDialect::Grok,
        )
        .await?;
        assert!(activities(&result).is_empty());
        assert!(matches!(
            result.last(),
            Some(Ok(ResponseEvent::Completed { .. }))
        ));
    }
    let local = json!({"type":"function_call","id":"x-1","call_id":"shared-call","name":"x_keyword_search","arguments":"{}"});
    let result = run_web_events(
        vec![
            added(/*index*/ 0),
            item_frame(
                "response.output_item.added",
                /*index*/ 1,
                local.clone(),
            ),
            item_frame("response.output_item.done", /*index*/ 1, local),
            done(/*index*/ 0),
            completed(),
        ],
        ApiDialect::Grok,
    )
    .await?;
    assert!(activities(&result).is_empty());
    Ok(())
}

#[tokio::test]
async fn grok_x_activity_rejects_conflicting_identity_payload_and_lifecycle() -> Result<()> {
    for case in [
        "wrong_event_id",
        "missing_event_id",
        "missing_index",
        "missing_input",
        "wrong_input_type",
        "conflicting_input",
        "late_delta",
        "changed_name",
        "changed_call",
        "changed_namespace",
        "changed_item_id",
        "changed_input",
        "incomplete",
        "terminal_index",
        "oversized_binding",
    ] {
        let start = item_frame(
            "response.output_item.added",
            /*index*/ 1,
            x_item("x_keyword_search", "in_progress"),
        );
        let mut signal = input_done();
        match case {
            "wrong_event_id" => signal["item_id"] = json!("other"),
            "missing_event_id" => {
                signal.as_object_mut().unwrap().remove("item_id");
            }
            "missing_index" => {
                signal.as_object_mut().unwrap().remove("output_index");
            }
            "missing_input" => {
                signal.as_object_mut().unwrap().remove("input");
            }
            "wrong_input_type" => signal["input"] = json!({}),
            "oversized_binding" => signal["input"] = json!("a".repeat(1024 * 1024 + 1)),
            _ => {}
        }
        let mut events = vec![added(/*index*/ 0), start, signal];
        if case == "conflicting_input" {
            let mut different = input_done();
            different["input"] = json!("changed");
            events.push(different);
        }
        if case == "late_delta" {
            events.push(json!({"type":"response.custom_tool_call_input.delta","output_index":1,"item_id":"x-1","delta":"late"}));
        }
        let mut finished = x_item("x_keyword_search", "completed");
        match case {
            "changed_name" => finished["name"] = json!("x_semantic_search"),
            "changed_call" => finished["call_id"] = json!("other"),
            "changed_namespace" => finished["namespace"] = json!("functions"),
            "changed_item_id" => finished["id"] = json!("other"),
            "changed_input" => finished["input"] = json!("other"),
            "incomplete" => finished["status"] = json!("in_progress"),
            "terminal_index" => events.push(
                json!({"type":"response.completed","output_index":1,"response":{"id":"bad"}}),
            ),
            _ => {}
        }
        events.extend([
            item_frame("response.output_item.done", /*index*/ 1, finished),
            done(/*index*/ 0),
            completed(),
        ]);
        let result = run_web_events(events, ApiDialect::Grok).await?;
        assert!(
            !activities(&result)
                .iter()
                .any(|(_, _, _, state)| *state == SearchActivityState::Completed),
            "{case}"
        );
        assert_failed(result);
    }
    Ok(())
}

#[tokio::test]
async fn grok_x_activity_requires_supported_early_signal_without_losing_canonical_completion()
-> Result<()> {
    let events = vec![
        added(/*index*/ 0),
        item_frame(
            "response.output_item.added",
            /*index*/ 1,
            x_item("x_keyword_search", "in_progress"),
        ),
        item_frame(
            "response.output_item.done",
            /*index*/ 1,
            x_item("x_keyword_search", "completed"),
        ),
        done(/*index*/ 0),
        completed(),
    ];
    let result = run_web_events(events, ApiDialect::Grok).await?;
    assert!(activities(&result).is_empty());
    assert_eq!(
        result
            .iter()
            .filter(|event| matches!(
                event,
                Ok(ResponseEvent::OutputItemDone(
                    ResponseItem::CustomToolCall { .. }
                ))
            ))
            .count(),
        1
    );
    assert!(matches!(
        result.last(),
        Some(Ok(ResponseEvent::Completed { .. }))
    ));
    Ok(())
}

#[tokio::test]
async fn grok_x_activity_rejects_unsupported_start_becoming_hosted_at_completion() -> Result<()> {
    for case in ["unknown_name", "namespace", "missing_id"] {
        let mut start = x_item("x_keyword_search", "in_progress");
        match case {
            "unknown_name" => start["name"] = json!("x_unknown"),
            "namespace" => start["namespace"] = json!("local"),
            "missing_id" => {
                start.as_object_mut().unwrap().remove("id");
            }
            _ => unreachable!(),
        }
        let result = run_web_events(
            vec![
                added(/*index*/ 0),
                item_frame("response.output_item.added", /*index*/ 1, start),
                input_done(),
                item_frame(
                    "response.output_item.done",
                    /*index*/ 1,
                    x_item("x_keyword_search", "completed"),
                ),
                done(/*index*/ 0),
                completed(),
            ],
            ApiDialect::Grok,
        )
        .await?;
        assert!(activities(&result).is_empty());
        assert_failed(result);
    }
    let mut start = x_item("x_keyword_search", "in_progress");
    start.as_object_mut().unwrap().remove("status");
    let result = run_web_events(
        vec![
            added(/*index*/ 0),
            item_frame("response.output_item.added", /*index*/ 1, start),
            input_done(),
            item_frame(
                "response.output_item.done",
                /*index*/ 1,
                x_item("x_keyword_search", "completed"),
            ),
            done(/*index*/ 0),
            completed(),
        ],
        ApiDialect::Grok,
    )
    .await?;
    assert_eq!(
        activities(&result).len(),
        2,
        "reference custom start has no status field"
    );
    assert!(matches!(
        result.last(),
        Some(Ok(ResponseEvent::Completed { .. }))
    ));
    Ok(())
}
