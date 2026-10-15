use super::*;
use codex_history::CodexHarnessMetadata;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn closed_turn_input_returns_snapshot_and_rejects_later_appends() {
    let input_queue = InputQueue::new();
    let active_turn = ActiveTurn::default();
    let turn_state = active_turn.turn_state.as_ref();
    let first = TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: "accepted before completion".to_string(),
            text_elements: Vec::new(),
        }],
        client_id: Some("client-input".to_string()),
        metadata: UserInputMetadata {
            acceptance_order: Some(7),
            origin: codex_history::UserInputOrigin::Heartbeat,
        },
    };
    let second = TurnInput::ResponseItem(ResponseItemEnvelope {
        item: ResponseItem::Other,
        metadata: Some(CodexHarnessMetadata {
            client_authored: true,
            ..Default::default()
        }),
    });
    input_queue
        .extend_pending_input_for_turn_state(turn_state, vec![first.clone()])
        .await;
    input_queue
        .extend_pending_input_and_accept_mailbox_delivery_for_turn_state(
            turn_state,
            vec![second.clone()],
        )
        .await
        .expect("open queue accepts input");
    assert_eq!(
        input_queue
            .take_pending_input_for_turn_state(turn_state)
            .await,
        vec![first.clone(), second.clone()]
    );
    turn_state
        .lock()
        .await
        .set_mailbox_delivery_phase(MailboxDeliveryPhase::NextTurn);
    let (activity_rx, pending_activity) = input_queue.subscribe_activity(Some(turn_state)).await;
    assert_eq!(pending_activity, None);

    for late in [vec![second, first], Vec::new()] {
        assert_eq!(
            input_queue
                .extend_pending_input_and_accept_mailbox_delivery_for_turn_state(
                    turn_state,
                    late.clone(),
                )
                .await,
            Err(late)
        );
        assert!(!activity_rx.has_changed().unwrap());
        let state = turn_state.lock().await;
        assert!(state.pending_input.is_closed());
        assert!(state.pending_input.is_empty());
        assert!(!state.accepts_mailbox_delivery_for_current_turn());
    }

    input_queue.clear_pending(&active_turn).await;
    assert!(turn_state.lock().await.pending_input.is_closed());
    assert!(
        input_queue
            .take_pending_input_for_turn_state(turn_state)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn sampling_drain_keeps_turn_input_open() {
    let input_queue = InputQueue::new();
    let active_turn = ActiveTurn::default();
    let turn_state = Arc::clone(&active_turn.turn_state);
    let active = Mutex::new(Some(active_turn));
    let item = TurnInput::ResponseItem(ResponseItem::Other.into());
    input_queue
        .extend_pending_input_for_turn_state(turn_state.as_ref(), vec![item.clone()])
        .await;
    assert_eq!(
        input_queue.get_pending_input(&active).await.0,
        vec![item.clone()]
    );
    assert!(!turn_state.lock().await.pending_input.is_closed());
    turn_state
        .lock()
        .await
        .set_mailbox_delivery_phase(MailboxDeliveryPhase::NextTurn);
    let (mut activity_rx, _) = input_queue
        .subscribe_activity(Some(turn_state.as_ref()))
        .await;
    input_queue
        .extend_pending_input_and_accept_mailbox_delivery_for_turn_state(
            turn_state.as_ref(),
            vec![item.clone()],
        )
        .await
        .expect("sampling drain leaves queue open");
    assert!(
        turn_state
            .lock()
            .await
            .accepts_mailbox_delivery_for_current_turn()
    );
    assert!(activity_rx.has_changed().unwrap());
    assert_eq!(*activity_rx.borrow_and_update(), InputQueueActivity::Steer);
    assert_eq!(
        input_queue
            .take_pending_input_for_turn_state(turn_state.as_ref())
            .await,
        vec![item]
    );
}
