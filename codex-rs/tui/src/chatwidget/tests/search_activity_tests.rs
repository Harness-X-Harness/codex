//! Public notification composition: previews never take ownership of assistant output.

use super::*;
use codex_app_server_protocol::SearchActivityNotification;
use codex_app_server_protocol::SearchActivityState;
use codex_app_server_protocol::SearchActivityState::Cleared;
use codex_app_server_protocol::SearchActivityState::Completed;
use codex_app_server_protocol::SearchActivityState::Running;
use codex_app_server_protocol::WebSearchAction;
use codex_app_server_protocol::WebSearchItem;
use pretty_assertions::assert_eq;

fn activity(
    chat: &ChatWidget,
    attempt_id: u64,
    output_index: u64,
    item_id: &str,
    state: SearchActivityState,
) -> ServerNotification {
    ServerNotification::SearchActivity(SearchActivityNotification {
        kind: codex_app_server_protocol::SearchActivityKind::Web,
        thread_id: chat.thread_id.expect("thread").to_string(),
        turn_id: "turn-1".into(),
        attempt_id,
        output_index,
        item_id: item_id.into(),
        state,
    })
}

fn send_activity(
    chat: &mut ChatWidget,
    attempt_id: u64,
    output_index: u64,
    item_id: &str,
    state: SearchActivityState,
) {
    let notification = activity(chat, attempt_id, output_index, item_id, state);
    chat.handle_server_notification(notification, /*replay_kind*/ None);
}

fn web_item(item_id: &str) -> ThreadItem {
    ThreadItem::WebSearch(WebSearchItem {
        id: item_id.into(),
        query: "canonical query".into(),
        action: Some(WebSearchAction::Search {
            query: Some("canonical query".into()),
            queries: None,
        }),
        results: None,
    })
}

fn complete_web(chat: &mut ChatWidget, item_id: &str) {
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: chat.thread_id.expect("thread").to_string(),
            turn_id: "turn-1".into(),
            item: web_item(item_id),
            completed_at_ms: 0,
        }),
        /*replay_kind*/ None,
    );
}

#[tokio::test]
async fn search_activity_settles_beside_unbroken_assistant_stream_then_promotes_once() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.local_settings.tui.animations = false;
    handle_turn_started(&mut chat, "turn-1");
    drain_insert_history(&mut rx);
    handle_agent_message_delta(&mut chat, "A");
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 80).unwrap()), @"
    • A

    • Searching the web
    ");
    assert!(chat.stream_controller.is_some());
    assert!(chat.active_cell_is_stream_tail());
    assert!(render_bottom_popup(&chat, /*width*/ 80).contains("Searching the web"));
    assert!(drain_insert_history(&mut rx).is_empty());

    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Completed,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
    );
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: chat.thread_id.unwrap().to_string(),
            turn_id: "turn-1".into(),
            item: web_item("web"),
            started_at_ms: 0,
        }),
        /*replay_kind*/ None,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 18).unwrap()), @"
    • A

    • Web search
      completed
    ");
    assert!(chat.stream_controller.is_some());
    assert!(chat.active_cell_is_stream_tail());
    assert!(render_bottom_popup(&chat, /*width*/ 80).contains("Web search completed"));
    assert!(drain_insert_history(&mut rx).is_empty());
    handle_agent_message_delta(&mut chat, "B");
    complete_assistant_message(&mut chat, "msg-1", "AB", /*phase*/ None);
    assert_eq!(chat.transcript.last_agent_source.as_deref(), Some("AB"));
    complete_web(&mut chat, "web");
    let mut assistant_sources = Vec::new();
    let mut web_rows = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::ConsolidateAgentMessage { source, .. } => assistant_sources.push(source),
            AppEvent::InsertHistoryCell(cell) if cell.as_any().is::<WebSearchCell>() => {
                web_rows.extend(cell.raw_lines().iter().map(ToString::to_string));
            }
            _ => {}
        }
    }
    assert_eq!(assistant_sources, vec!["AB"]);
    assert_eq!(web_rows, vec!["Searched the web for canonical query"]);
    assert!(chat.transcript.search_activity.is_empty());
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
    );
    assert!(chat.transcript.search_activity.is_empty());
}

#[tokio::test]
async fn search_activity_retry_reuse_and_late_attempts_cannot_reopen_previews() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "turn-1");
    drain_insert_history(&mut rx);
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "same", Running,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "second", Running,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "same", Completed,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 80).unwrap()), @"
    • Web search completed

    • Searching the web
    ");
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "same", Cleared,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 3, "late", Running,
    );
    assert!(chat.transcript.search_activity.is_empty());
    send_activity(
        &mut chat, /*attempt_id*/ 2, /*output_index*/ 1, "same", Running,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "same", Completed,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "same", Cleared,
    );
    assert_eq!(
        chat.transcript
            .search_activity
            .cells()
            .map(HistoryCell::raw_lines)
            .collect::<Vec<_>>(),
        vec![vec![Line::from("Searching the web")]]
    );
    send_activity(
        &mut chat, /*attempt_id*/ 3, /*output_index*/ 2, "new", Running,
    );
    assert_eq!(
        chat.transcript
            .search_activity
            .cells()
            .map(crate::history_cell::SearchActivityCell::item_id)
            .collect::<Vec<_>>(),
        vec!["new"]
    );
    assert!(drain_insert_history(&mut rx).is_empty());
}

#[tokio::test]
async fn search_activity_ignores_wrong_thread_and_invalid_binding_then_clears_on_error() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "turn-1");
    let mut wrong_thread = activity(
        &chat, /*attempt_id*/ 1, /*output_index*/ 1, "wrong", Running,
    );
    if let ServerNotification::SearchActivity(activity) = &mut wrong_thread {
        activity.thread_id = ThreadId::new().to_string();
    }
    chat.handle_server_notification(wrong_thread, /*replay_kind*/ None);
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "web", Running,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "other", Completed,
    );
    send_activity(
        &mut chat,
        /*attempt_id*/ 1,
        /*output_index*/ 3,
        &"x".repeat(1025),
        Running,
    );
    assert_eq!(
        chat.transcript
            .search_activity
            .cells()
            .map(HistoryCell::raw_lines)
            .collect::<Vec<_>>(),
        vec![vec![Line::from("Searching the web")]]
    );
    handle_error(&mut chat, "stream failed", /*codex_error_info*/ None);
    assert!(chat.transcript.search_activity.is_empty());
    send_activity(
        &mut chat, /*attempt_id*/ 2, /*output_index*/ 1, "web", Running,
    );
    assert!(chat.transcript.search_activity.is_empty());
    assert!(
        drain_insert_history_with(&mut rx, HistoryCell::raw_lines)
            .iter()
            .flatten()
            .any(|line| line.to_string().contains("stream failed"))
    );
}

#[tokio::test]
async fn search_activity_distinct_retained_ids_and_large_index_stay_bounded() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "turn-1");
    drain_insert_history(&mut rx);
    // The producer rejects same-turn reuse after retention; distinct canonical
    // operations therefore use distinct IDs even after preview eviction.
    for index in [1, 2, (1_u64 << 53) - 1] {
        let item_id = format!("web-{index}");
        send_activity(&mut chat, /*attempt_id*/ 1, index, &item_id, Running);
        assert_eq!(chat.transcript.search_activity.cells().count(), 1);
        complete_web(&mut chat, &item_id);
        assert!(chat.transcript.search_activity.is_empty());
    }
    assert_eq!(
        drain_insert_history_with(&mut rx, HistoryCell::raw_lines).len(),
        3
    );
    // A new attempt gets a fresh bounded registry, even after the largest supported index.
    for index in 0..70 {
        send_activity(
            &mut chat,
            /*attempt_id*/ 2,
            index,
            &format!("pending-{index}"),
            Running,
        );
    }
    assert_eq!(chat.transcript.search_activity.cells().count(), 65);
}

#[tokio::test]
async fn search_activity_turn_end_and_replay_discard_transient_success() {
    for interrupted in [false, true] {
        let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.thread_id = Some(ThreadId::new());
        handle_turn_started(&mut chat, "turn-1");
        send_activity(
            &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
        );
        send_activity(
            &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Completed,
        );
        if interrupted {
            handle_turn_interrupted(&mut chat, "turn-1");
        } else {
            handle_turn_completed(&mut chat, "turn-1", /*duration_ms*/ None);
        }
        send_activity(
            &mut chat, /*attempt_id*/ 2, /*output_index*/ 1, "web", Running,
        );
        assert!(chat.transcript.search_activity.is_empty());
        let rows = drain_insert_history_with(&mut rx, HistoryCell::raw_lines);
        assert!(
            !rows
                .iter()
                .flatten()
                .any(|line| line.to_string().contains("Web search completed"))
        );
        handle_turn_started(&mut chat, "turn-2");
        send_activity(
            &mut chat, /*attempt_id*/ 3, /*output_index*/ 1, "web", Running,
        );
        assert!(chat.transcript.search_activity.is_empty());
        let mut notification = activity(
            &chat, /*attempt_id*/ 4, /*output_index*/ 1, "web", Running,
        );
        if let ServerNotification::SearchActivity(activity) = &mut notification {
            activity.turn_id = "turn-2".into();
        }
        chat.handle_server_notification(notification, Some(ReplayKind::ResumeInitialMessages));
        assert!(chat.transcript.search_activity.is_empty());
        chat.replay_thread_item(
            web_item("web"),
            "turn-2".into(),
            ReplayKind::ResumeInitialMessages,
        );
        assert_eq!(
            drain_insert_history_with(&mut rx, HistoryCell::raw_lines)
                .into_iter()
                .flatten()
                .filter(|line| line.to_string().contains("canonical query"))
                .count(),
            1
        );
    }
}

fn send_x_activity(
    chat: &mut ChatWidget,
    attempt_id: u64,
    output_index: u64,
    item_id: &str,
    state: SearchActivityState,
) {
    let mut notification = activity(chat, attempt_id, output_index, item_id, state);
    if let ServerNotification::SearchActivity(activity) = &mut notification {
        activity.kind = codex_app_server_protocol::SearchActivityKind::X;
    }
    chat.handle_server_notification(notification, /*replay_kind*/ None);
}

fn x_item(id: &str) -> ThreadItem {
    ThreadItem::XSearch(codex_app_server_protocol::XSearchItem {
        id: id.into(),
        call_id: "shared-call".into(),
        name: "x_keyword_search".into(),
        input: "exact input\n".into(),
    })
}

#[tokio::test]
async fn search_activity_mixed_web_x_keeps_truthful_labels_and_assistant_ownership() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.local_settings.tui.animations = false;
    handle_turn_started(&mut chat, "turn-1");
    drain_insert_history(&mut rx);
    handle_agent_message_delta(&mut chat, "A");
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Running,
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "x", Running,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 80).unwrap()), @"
    • A

    • Searching the web

    • Searching X
    ");
    assert!(render_bottom_popup(&chat, /*width*/ 80).contains("Searching X"));
    // A mismatched kind cannot settle the other search sharing this output index.
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "x", Completed,
    );
    assert!(
        chat.transcript
            .search_activity
            .cells()
            .last()
            .unwrap()
            .raw_lines()[0]
            .to_string()
            .contains("Searching X")
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "x", Completed,
    );
    send_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "web", Completed,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 80).unwrap()), @"
    • A

    • Web search completed

    • X search completed
    ");
    assert!(chat.stream_controller.is_some());
    assert!(chat.active_cell_is_stream_tail());
    assert!(drain_insert_history(&mut rx).is_empty());
    handle_agent_message_delta(&mut chat, "B");
    complete_assistant_message(&mut chat, "msg-1", "AB", /*phase*/ None);
    complete_web(&mut chat, "web");
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: chat.thread_id.unwrap().to_string(),
            turn_id: "turn-1".into(),
            item: x_item("x"),
            completed_at_ms: 0,
        }),
        /*replay_kind*/ None,
    );
    assert!(chat.transcript.search_activity.is_empty());
    assert_eq!(chat.transcript.last_agent_source.as_deref(), Some("AB"));
    let rows = drain_insert_history_with(&mut rx, HistoryCell::raw_lines)
        .into_iter()
        .flatten()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter()
            .filter(|line| line.contains("Searched X"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["• Searched X (x_keyword_search)"]
    );
    assert_eq!(
        rows.iter()
            .filter(|line| line.contains("Searched the web"))
            .count(),
        1
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 2, "x", Running,
    );
    assert!(chat.transcript.search_activity.is_empty());
}

#[tokio::test]
async fn search_activity_x_cleanup_retry_late_turn_and_cold_replay_never_resurrect_preview() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "turn-1");
    drain_insert_history(&mut rx);
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "x", Running,
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "x", Completed,
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "x", Cleared,
    );
    assert!(chat.transcript.search_activity.is_empty());
    send_x_activity(
        &mut chat, /*attempt_id*/ 2, /*output_index*/ 1, "x", Running,
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "x", Completed,
    );
    send_x_activity(
        &mut chat, /*attempt_id*/ 1, /*output_index*/ 1, "x", Cleared,
    );
    insta::assert_snapshot!(lines_to_single_string(&chat.active_cell_transcript_lines(/*width*/ 80).unwrap()), @"• Searching X");
    handle_turn_interrupted(&mut chat, "turn-1");
    assert!(chat.transcript.search_activity.is_empty());
    handle_turn_started(&mut chat, "turn-2");
    send_x_activity(
        &mut chat, /*attempt_id*/ 3, /*output_index*/ 1, "x", Running,
    );
    assert!(chat.transcript.search_activity.is_empty());
    chat.replay_thread_item(
        x_item("x"),
        "turn-1".into(),
        ReplayKind::ResumeInitialMessages,
    );
    assert!(chat.transcript.search_activity.is_empty());
    let rows = drain_insert_history_with(&mut rx, HistoryCell::raw_lines)
        .into_iter()
        .flatten()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter()
            .filter(|line| line.contains("Searched X"))
            .count(),
        1
    );
    assert!(
        !rows
            .iter()
            .any(|line| line.contains("Searching X") || line.contains("X search completed"))
    );
}
