use crate::session::TurnInput;
use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use crate::session::tests::make_session_and_context_with_rx;
use crate::state::ActiveTurn;
use crate::state::MailboxDeliveryPhase;
use codex_features::Feature;
use codex_history::CodexHarnessMetadata;
use codex_history::ResponseItemEnvelope;
use codex_history::RolloutItem;
use codex_login::CodexAuth;
use codex_protocol::ResponseItemId;
use codex_protocol::models::ConfigurationReasoning;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[tokio::test]
async fn generic_inject_preserves_envelopes_and_reserved_open_slot() {
    let (session, _turn_context, _rx_event) = make_session_and_context_with_rx().await;
    let active_turn = ActiveTurn::default();
    let turn_state = Arc::clone(&active_turn.turn_state);
    *session.active_turn.lock().await = Some(active_turn);
    let input = vec![ResponseItemEnvelope {
        item: ResponseItem::Other,
        metadata: Some(CodexHarnessMetadata {
            client_authored: true,
            ..Default::default()
        }),
    }];
    assert_eq!(
        session
            .inject_hook_context_if_running(vec![ResponseItem::Other])
            .await,
        Err(vec![ResponseItem::Other])
    );
    let (mut activity_rx, _) = session
        .input_queue
        .subscribe_activity(Some(turn_state.as_ref()))
        .await;
    assert_eq!(session.inject_if_running(input.clone()).await, Ok(()));
    assert!(activity_rx.has_changed().unwrap());
    activity_rx.borrow_and_update();
    assert_eq!(
        session
            .input_queue
            .take_pending_input_for_turn_state(turn_state.as_ref())
            .await,
        input
            .iter()
            .cloned()
            .map(TurnInput::ResponseItem)
            .collect::<Vec<_>>()
    );
    turn_state
        .lock()
        .await
        .set_mailbox_delivery_phase(MailboxDeliveryPhase::NextTurn);
    assert_eq!(session.inject_if_running(input.clone()).await, Err(input));
    assert!(!activity_rx.has_changed().unwrap());
    assert!(
        !turn_state
            .lock()
            .await
            .accepts_mailbox_delivery_for_current_turn()
    );
    assert!(turn_state.lock().await.pending_input.is_empty());
    let active = session.active_turn.lock().await;
    assert!(active.as_ref().unwrap().task.is_none());
    assert!(Arc::ptr_eq(
        &active.as_ref().unwrap().turn_state,
        &turn_state
    ));
}

#[tokio::test]
async fn generic_inject_rejects_without_converting_original_input() {
    #[derive(Debug, PartialEq)]
    struct UnconvertedInput(String);

    impl From<UnconvertedInput> for ResponseItemEnvelope {
        fn from(_: UnconvertedInput) -> Self {
            panic!("rejected input must not be converted")
        }
    }

    let (session, _turn_context, _rx_event) = make_session_and_context_with_rx().await;
    assert_eq!(
        session
            .inject_if_running(vec![UnconvertedInput("idle".to_string())])
            .await,
        Err(vec![UnconvertedInput("idle".to_string())])
    );
    let active_turn = ActiveTurn::default();
    session
        .input_queue
        .take_pending_input_for_turn_state(active_turn.turn_state.as_ref())
        .await;
    *session.active_turn.lock().await = Some(active_turn);
    assert_eq!(
        session
            .inject_if_running(vec![UnconvertedInput("closed".to_string())])
            .await,
        Err(vec![UnconvertedInput("closed".to_string())])
    );
}

#[test_case::test_case(false; "plain history")]
#[test_case::test_case(true; "client provenance")]
#[tokio::test]
async fn client_inject_records_history_after_turn_input_is_closed(retain_client_messages: bool) {
    let (session, turn_context, rx_event) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("Test API Key"),
        Vec::new(),
        |config| {
            if retain_client_messages {
                config
                    .features
                    .enable(Feature::RetainClientDeveloperMessages)
                    .unwrap();
            }
        },
    )
    .await;
    let active_turn = ActiveTurn::default();
    let turn_state = Arc::clone(&active_turn.turn_state);
    *session.active_turn.lock().await = Some(active_turn);
    session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    let items = [
        ("developer", "Preserve this client's instruction."),
        ("user", "Stop. Do not change any files."),
    ]
    .into_iter()
    .map(|(role, text)| {
        let mut item = ResponseItem::Message {
            id: Some(ResponseItemId::with_suffix("msg", role)),
            role: role.to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        };
        item.set_turn_id_if_missing("client-origin-turn");
        item.set_create_time_if_missing(123.into());
        item
    })
    .collect::<Vec<_>>();
    session
        .inject_client_response_items(items.clone(), &turn_context)
        .await;
    let history = session.clone_history().await;
    assert_eq!(
        history
            .conversation_history_snapshot()
            .user_message_revision(),
        1
    );
    let recorded = history.into_annotated_items();
    assert_eq!(
        recorded
            .iter()
            .map(|envelope| envelope.item.clone())
            .collect::<Vec<_>>(),
        items
    );
    assert_eq!(
        recorded[0]
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.client_authored),
        retain_client_messages
    );
    assert!(
        !recorded[1]
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.client_authored)
    );
    let rollout_items = recorded
        .iter()
        .cloned()
        .map(RolloutItem::ResponseItem)
        .collect::<Vec<_>>();
    let reconstructed = session
        .reconstruct_history_from_rollout(&turn_context, &rollout_items)
        .await;
    assert_eq!(reconstructed.history, recorded);
    let mut raw_items = Vec::new();
    while let Ok(event) = rx_event.try_recv() {
        match event.msg {
            EventMsg::RawResponseItem(event) => raw_items.push(event.item),
            EventMsg::TurnStarted(_) => panic!("history fallback must not start a turn"),
            _ => {}
        }
    }
    assert_eq!(raw_items, items);
    assert!(turn_state.lock().await.pending_input.is_empty());
    let active = session.active_turn.lock().await;
    assert!(active.as_ref().unwrap().task.is_none());
    assert!(Arc::ptr_eq(
        &active.as_ref().unwrap().turn_state,
        &turn_state
    ));
}

#[tokio::test]
async fn no_new_turn_inject_records_history_after_turn_input_is_closed() {
    let (session, turn_context, _rx_event) = make_session_and_context_with_rx().await;
    let active_turn = ActiveTurn::default();
    let turn_state = Arc::clone(&active_turn.turn_state);
    session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    *session.active_turn.lock().await = Some(active_turn);
    let mut item: ResponseItem = serde_json::from_value(serde_json::json!({
        "type": "function_call_output",
        "name": "shell",
        "output": "completed shell output"
    }))
    .unwrap();
    item.set_id(Some(ResponseItemId::with_suffix(
        "fco",
        "closed-shell-output",
    )));
    item.set_turn_id_if_missing("shell-origin-turn");
    item.set_create_time_if_missing(123.into());
    session
        .inject_no_new_turn(vec![item.clone()], Some(&turn_context))
        .await;
    let recorded = session.clone_history().await.into_annotated_items();
    assert_eq!(
        recorded
            .iter()
            .map(|envelope| envelope.item.clone())
            .collect::<Vec<_>>(),
        vec![item]
    );
    assert!(turn_state.lock().await.pending_input.is_empty());
    let active = session.active_turn.lock().await;
    assert!(active.as_ref().unwrap().task.is_none());
    assert!(Arc::ptr_eq(
        &active.as_ref().unwrap().turn_state,
        &turn_state
    ));
}

#[tokio::test]
async fn harness_authored_configuration_updates_preserve_metadata_and_resume() {
    let (session, turn_context, rx_event) = make_session_and_context_with_rx().await;
    assert!(!session.enabled(Feature::RetainClientDeveloperMessages));

    let mut expected = ResponseItemEnvelope {
        item: ResponseItem::ConfigurationUpdate {
            reasoning: ConfigurationReasoning {
                effort: ReasoningEffort::High,
            },
        },
        metadata: Some(CodexHarnessMetadata {
            harness_authored_configuration: true,
            ..Default::default()
        }),
    };
    session
        .record_annotated_conversation_items(
            &turn_context,
            turn_context.model_info(),
            vec![expected.clone()],
        )
        .await;

    expected.metadata.as_mut().unwrap().mcp_attribution = Some(
        session
            .services
            .executed_tool_calls
            .mcp_attribution_snapshot(),
    );

    let recorded = session.clone_history().await.into_annotated_items();
    assert_eq!(recorded, vec![expected.clone()]);
    let mut raw_items = Vec::new();
    while let Ok(event) = rx_event.try_recv() {
        if let EventMsg::RawResponseItem(event) = event.msg {
            raw_items.push(event.item);
        }
    }
    assert_eq!(raw_items, vec![expected.item]);

    let rollout_items = recorded
        .iter()
        .cloned()
        .map(RolloutItem::ResponseItem)
        .collect::<Vec<_>>();
    let reconstructed = session
        .reconstruct_history_from_rollout(&turn_context, &rollout_items)
        .await;
    assert_eq!(reconstructed.history, recorded);
}
