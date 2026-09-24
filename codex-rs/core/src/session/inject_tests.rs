use std::sync::Arc;

use crate::session::tests::make_session_and_context_with_rx;
use codex_features::Feature;
use codex_history::CodexHarnessMetadata;
use codex_history::ResponseItemEnvelope;
use codex_history::RolloutItem;
use codex_protocol::models::ConfigurationReasoning;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn harness_authored_configuration_updates_preserve_metadata_and_resume() {
    let (session, turn_context, rx_event) = make_session_and_context_with_rx().await;
    assert!(!session.enabled(Feature::RetainClientDeveloperMessages));

    let expected = ResponseItemEnvelope {
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

#[tokio::test]
async fn client_inject_records_history_after_turn_input_is_closed() {
    let (session, turn_context, _rx_event) = make_session_and_context_with_rx().await;
    let turn_state = {
        let mut active = session.active_turn.lock().await;
        let active_turn = active.get_or_insert_with(crate::state::ActiveTurn::default);
        assert!(active_turn.task.is_none());
        Arc::clone(&active_turn.turn_state)
    };
    assert!(
        session
            .input_queue
            .take_pending_input_for_turn_state(turn_state.as_ref())
            .await
            .is_empty()
    );

    let text = "Stop. Do not change any files.";
    session
        .inject_client_response_items(
            vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: text.to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }],
            &turn_context,
        )
        .await;

    assert!(session.active_turn.lock().await.is_some());
    assert!(
        session
            .input_queue
            .take_pending_input_for_turn_state(turn_state.as_ref())
            .await
            .is_empty()
    );
    let history = session.clone_history().await;
    assert_eq!(
        history
            .conversation_history_snapshot()
            .user_message_revision(),
        1
    );
    assert!(history.into_annotated_items().iter().any(|envelope| {
        matches!(
            &envelope.item,
            ResponseItem::Message { role, content, .. }
                if role == "user"
                    && content.iter().any(|content| matches!(
                        content,
                        ContentItem::InputText { text: recorded } if recorded == text
                    ))
        )
    }));
}
