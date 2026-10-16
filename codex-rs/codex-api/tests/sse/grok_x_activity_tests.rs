//! Product retirement controls. These are failures, never positive Hosted X stories.
use super::*;

#[tokio::test]
async fn retired_hosted_x_fails_before_activity_or_canonical_delivery() -> Result<()> {
    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        let item = json!({"type":"custom_tool_call", "id":"x-1", "call_id":"call-x",
            "name":name, "status":"in_progress", "input":"{}"});
        let events = run_web_events(
            vec![
                added(/*index*/ 0),
                item_frame("response.output_item.added", /*index*/ 1, item),
                done(/*index*/ 0),
                completed(),
            ],
            ApiDialect::Grok,
        )
        .await?;
        assert!(events.iter().all(|event| !matches!(
            event,
            Ok(ResponseEvent::SearchActivity { .. })
                | Ok(ResponseEvent::OutputItemAdded(
                    codex_protocol::models::ResponseItem::CustomToolCall { .. }
                ))
                | Ok(ResponseEvent::OutputItemDone(
                    codex_protocol::models::ResponseItem::CustomToolCall { .. }
                ))
        )));
        assert!(events.iter().any(|event| matches!(event, Err(error) if error.to_string().contains("Hosted X Search is retired"))));
        assert_failed(events);
    }
    Ok(())
}

#[tokio::test]
async fn retired_hosted_x_does_not_change_stock_or_local_function_wire() -> Result<()> {
    let item = json!({"type":"function_call", "id":"local", "call_id":"local-call",
        "name":"x_keyword_search", "arguments":"{}"});
    let frames = vec![
        item_frame("response.output_item.added", /*index*/ 0, item.clone()),
        item_frame("response.output_item.done", /*index*/ 0, item),
        completed(),
    ];
    assert!(
        run_events(frames, ApiDialect::Grok)
            .await?
            .iter()
            .all(Result::is_ok)
    );
    let hosted = json!({"type":"custom_tool_call", "id":"x", "call_id":"call-x",
        "name":"x_keyword_search", "status":"completed", "input":"{}"});
    assert!(
        run_events(
            vec![
                item_frame(
                    "response.output_item.added",
                    /*index*/ 0,
                    hosted.clone()
                ),
                item_frame("response.output_item.done", /*index*/ 0, hosted),
                completed(),
            ],
            ApiDialect::OpenAi
        )
        .await?
        .iter()
        .all(Result::is_ok)
    );
    Ok(())
}
