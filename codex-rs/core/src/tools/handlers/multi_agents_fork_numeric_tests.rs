use super::*;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::ThreadHistoryMode;
use std::collections::HashSet;
use std::sync::Mutex as StdMutex;

fn markers<'a>(items: impl Iterator<Item = &'a ResponseItem>) -> Vec<(String, String)> {
    items
        .filter_map(|item| {
            let ResponseItem::Message { role, content, .. } = item else {
                return None;
            };
            let [ContentItem::InputText { text } | ContentItem::OutputText { text }] =
                content.as_slice()
            else {
                return None;
            };
            text.starts_with("c3-")
                .then(|| (role.clone(), text.clone()))
        })
        .collect()
}

#[tokio::test]
async fn multi_agent_v2_spawn_numeric_fork_selects_persisted_recent_turn_with_role_override() {
    for token in ["1.0", "1e0", r#""1""#] {
        let (mut session, mut turn) = make_session_and_context().await;
        let role_name = install_role_with_model_override(&mut turn).await;
        let mut config = (*turn.config).clone();
        config
            .features
            .enable(Feature::MultiAgentV2)
            .expect("test config should allow feature update");
        set_turn_config(&mut turn, config);
        let parent_provider_id = turn.config.model_provider_id.clone();
        let manager = thread_manager();
        let root = manager
            .start_thread(StartThreadOptions {
                history_mode: Some(ThreadHistoryMode::Legacy),
                ..StartThreadOptions::new((*turn.config).clone())
            })
            .await
            .expect("root thread should start");
        assert_eq!(
            root.thread.config_snapshot().await.history_mode,
            ThreadHistoryMode::Legacy
        );
        set_agent_control(
            &mut session,
            root.thread
                .session
                .services
                .local_agent_runtime
                .control(root.thread.session.session_id()),
        );
        session.thread_id = root.thread_id;

        let expected_parent = [
            ("user", "c3-old-user"),
            ("assistant", "c3-old-final"),
            ("user", "c3-recent-user"),
            ("assistant", "c3-recent-final"),
        ]
        .map(|(role, text)| (role.to_string(), text.to_string()));
        for pair in expected_parent.chunks(2) {
            let context = root.thread.session.new_default_turn().await;
            let items = pair
                .iter()
                .map(|(role, text)| ResponseItem::Message {
                    id: None,
                    role: role.clone(),
                    content: vec![if role == "user" {
                        ContentItem::InputText { text: text.clone() }
                    } else {
                        ContentItem::OutputText { text: text.clone() }
                    }],
                    phase: (role == "assistant").then_some(MessagePhase::FinalAnswer),
                    internal_chat_message_metadata_passthrough: None,
                })
                .collect::<Vec<_>>();
            root.thread
                .session
                .record_conversation_items(context.as_ref(), context.model_info(), &items)
                .await;
        }
        let parent_history = root.thread.session.clone_history().await;
        assert_eq!(
            markers(parent_history.raw_items()),
            expected_parent,
            "{token}"
        );

        let session = Arc::new(session);
        let turn = Arc::new(turn);
        let role_json = serde_json::to_string(&role_name).expect("role should serialize");
        let output = SpawnAgentHandlerV2::default()
            .handle(invocation(
                session.clone(),
                turn.clone(),
                "spawn_agent",
                ToolPayload::Function {
                    arguments: format!(
                        r#"{{"message":"inspect this repo","task_name":"numeric_fork","agent_type":{role_json},"fork_turns":{token}}}"#,
                    ),
                },
            ))
            .await
            .expect("partial fork should select history and allow a role override");
        let (content, success) = expect_text_output(output);
        let result: serde_json::Value =
            serde_json::from_str(&content).expect("spawn result should be JSON");
        assert_eq!(success, Some(true));
        assert_eq!(result["task_name"], "/root/numeric_fork");
        let child_id = session
            .services
            .local_agent_runtime
            .resolve_agent_reference(
                session.thread_id,
                &turn.session_source,
                result["task_name"]
                    .as_str()
                    .expect("task path should be a string"),
            )
            .await
            .expect("returned task path should resolve");
        let child = manager
            .get_thread(child_id)
            .await
            .expect("resolved child should be registered");
        let child_history = child.session.clone_history().await;
        assert_eq!(
            markers(child_history.raw_items()),
            expected_parent[2..],
            "{token}"
        );
        let parent_history = root.thread.session.clone_history().await;
        assert_eq!(
            markers(parent_history.raw_items()),
            expected_parent,
            "{token}"
        );
        let snapshot = child.config_snapshot().await;
        assert_eq!(
            (
                snapshot.model,
                snapshot.model_provider_id,
                snapshot.reasoning_effort
            ),
            (
                "gpt-5-role-override".to_string(),
                parent_provider_id,
                Some(ReasoningEffort::Minimal)
            )
        );
        assert_eq!(snapshot.session_source.get_agent_role(), Some(role_name));

        child
            .shutdown_and_wait()
            .await
            .expect("child should shut down");
        root.thread
            .shutdown_and_wait()
            .await
            .expect("parent should shut down");
    }
}

#[tokio::test]
async fn multi_agent_v2_spawn_numeric_fork_rejection_allocates_no_child() {
    let (mut session, mut turn) = make_session_and_context().await;
    let mut config = (*turn.config).clone();
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    set_turn_config(&mut turn, config);
    let generated = Arc::new(StdMutex::new(Vec::new()));
    let observer = Arc::clone(&generated);
    let manager = thread_manager().with_thread_id_generator(move || {
        let id = ThreadId::new();
        observer.lock().expect("ID observer should lock").push(id);
        id
    });
    let root = manager
        .start_thread(StartThreadOptions::new((*turn.config).clone()))
        .await
        .expect("root thread should start");
    set_agent_control(
        &mut session,
        root.thread
            .session
            .services
            .local_agent_runtime
            .control(root.thread.session.session_id()),
    );
    session.thread_id = root.thread_id;
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let generated_before = generated.lock().expect("ID observer should lock").clone();
    assert_eq!(generated_before, vec![root.thread_id]);
    let registered_before = manager
        .list_thread_ids()
        .await
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(registered_before, HashSet::from([root.thread_id]));
    let target = "/root/rejected_numeric_fork";
    assert!(
        session
            .services
            .local_agent_runtime
            .resolve_agent_reference(session.thread_id, &turn.session_source, target)
            .await
            .is_err()
    );
    let mut created = manager.subscribe_thread_created();
    let overflow = (usize::MAX as u128 + 1).to_string();
    for (token, expected_error) in [
        (
            "0",
            "fork_turns must be `none`, `all`, or a positive integer string",
        ),
        (
            "-0.0",
            "fork_turns must be `none`, `all`, or a positive integer string",
        ),
        ("-1", "expected an in-range whole number"),
        ("1.0000000000000001", "expected an in-range whole number"),
        (overflow.as_str(), "expected an in-range whole number"),
    ] {
        let Err(error) = SpawnAgentHandlerV2::default()
            .handle(invocation(
                session.clone(),
                turn.clone(),
                "spawn_agent",
                ToolPayload::Function {
                    arguments: format!(
                        r#"{{"message":"inspect this repo","task_name":"rejected_numeric_fork","fork_turns":{token}}}"#,
                    ),
                },
            ))
            .await
        else {
            panic!("invalid numeric fork should reject: {token}");
        };
        let FunctionCallError::RespondToModel(message) = error else {
            panic!("invalid numeric fork should return a model-facing error: {token}");
        };
        assert!(message.contains(expected_error), "{token}: {message}");
        assert_eq!(
            *generated.lock().expect("ID observer should lock"),
            generated_before,
            "{token}"
        );
        assert_eq!(
            manager
                .list_thread_ids()
                .await
                .into_iter()
                .collect::<HashSet<_>>(),
            registered_before,
            "{token}"
        );
        assert!(
            session
                .services
                .local_agent_runtime
                .resolve_agent_reference(session.thread_id, &turn.session_source, target)
                .await
                .is_err(),
            "{token}"
        );
        assert!(matches!(
            created.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
    root.thread
        .shutdown_and_wait()
        .await
        .expect("parent should shut down");
}
