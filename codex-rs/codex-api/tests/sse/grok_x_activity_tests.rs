//! Product retirement controls. These are failures, never positive Hosted X stories.
use super::*;
use pretty_assertions::assert_eq;

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
        let mut events = run_web_events(
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
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Ok(ResponseEvent::Completed { .. })))
        );
        let Some(Err(error @ ApiError::InvalidRequest { .. })) = events.pop() else {
            panic!("retired Hosted X must end with a terminal capability rejection");
        };
        let error = codex_api::map_api_error(error);
        assert!(matches!(
            error.details(),
            codex_protocol::error::CodexErrorDetails::InvalidRequest(message)
                if message == "Grok stream: Hosted X Search is retired and cannot be executed"
        ));
        assert_eq!(error.retry_delay(/*retry_count*/ 1), None);
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
