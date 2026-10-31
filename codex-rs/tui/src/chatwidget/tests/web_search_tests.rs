//! Canonical Web items drive live and replay presentation through the public protocol.

use super::*;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::WebSearchAction;
use codex_app_server_protocol::WebSearchItem;
use pretty_assertions::assert_eq;

fn completed_web_item() -> ThreadItem {
    ThreadItem::untimed(ThreadItemPayload::WebSearch(WebSearchItem {
        id: "web-item".into(),
        query: "canonical query".into(),
        action: Some(WebSearchAction::Search {
            query: Some("canonical query".into()),
            queries: None,
        }),
        results: None,
    }))
}

fn retained_web_rows(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    let mut rows = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event
            && cell.as_any().is::<WebSearchCell>()
        {
            rows.extend(cell.raw_lines().iter().map(ToString::to_string));
        }
    }
    rows
}

#[tokio::test]
async fn canonical_web_started_completed_retains_one_settled_cell() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.local_settings.tui.animations = false;
    handle_turn_started(&mut chat, "turn-1");
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".into(),
            item: ThreadItem::untimed(ThreadItemPayload::WebSearch(WebSearchItem {
                id: "web-item".into(),
                query: String::new(),
                action: None,
                results: None,
            })),
            started_at_ms: 0,
        }),
        /*replay_kind*/ None,
    );
    let active = chat
        .transcript
        .active_cell
        .as_ref()
        .and_then(|cell| cell.as_any().downcast_ref::<WebSearchCell>())
        .expect("canonical started Web item should be active");
    assert_eq!(active.call_id(), "web-item");
    assert_eq!(
        active.display_lines(/*width*/ 80)[0].to_string(),
        "• Browsing the web"
    );
    assert!(retained_web_rows(&mut rx).is_empty());

    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".into(),
            item: completed_web_item(),
            completed_at_ms: 1,
        }),
        /*replay_kind*/ None,
    );
    assert_eq!(
        retained_web_rows(&mut rx),
        vec!["Searched the web for canonical query"]
    );
    assert!(chat.transcript.active_cell.is_none());
}

#[tokio::test]
async fn canonical_web_completion_without_start_matches_replayed_notification() {
    for replay_kind in [
        None,
        Some(ReplayKind::ThreadSnapshot),
        Some(ReplayKind::ResumeInitialMessages),
    ] {
        let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
        let thread_id = ThreadId::new();
        chat.thread_id = Some(thread_id);
        chat.handle_server_notification(
            ServerNotification::ItemCompleted(ItemCompletedNotification {
                thread_id: thread_id.to_string(),
                turn_id: "turn-1".into(),
                item: completed_web_item(),
                completed_at_ms: 1,
            }),
            replay_kind,
        );
        assert_eq!(
            retained_web_rows(&mut rx),
            vec!["Searched the web for canonical query"]
        );
    }
}

#[tokio::test]
async fn canonical_web_thread_snapshot_retains_completed_search() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.replay_thread_turns(
        vec![Turn {
            id: "turn-1".into(),
            root_turn_id: None,
            items_view: codex_app_server_protocol::TurnItemsView::Full,
            items: vec![completed_web_item()],
            status: TurnStatus::Completed,
            error: None,
            started_at: Some(0),
            completed_at: Some(1),
            duration_ms: Some(1),
        }],
        ReplayKind::ThreadSnapshot,
    );
    assert_eq!(
        retained_web_rows(&mut rx),
        vec!["Searched the web for canonical query"]
    );
}
