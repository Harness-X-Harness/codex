//! Runtime spawn/notification witnesses, independent of C7's model-driven tool wire.
use super::*;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::ThreadHistoryMode;

async fn bind_grok(session: &mut crate::session::session::Session, turn: &mut TurnContext) {
    let catalog = serde_json::from_slice(
        &std::fs::read(
            codex_utils_cargo_bin::find_resource!("../../grok/dist/models.json").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let provider = create_model_provider(
        ModelProviderInfo {
            name: "Unrelated alias".into(),
            base_url: Some("http://127.0.0.1:9/grok".into()),
            wire_api: WireApi::GrokResponses,
            ..Default::default()
        },
        /*auth_manager*/ None,
    );
    let config = Arc::make_mut(&mut turn.config);
    config.model = Some("grok-4.6".into());
    config.model_provider_id = "configured-alias".into();
    config.model_provider = provider.info().clone();
    config.model_providers.insert(
        config.model_provider_id.clone(),
        config.model_provider.clone(),
    );
    config.model_catalog = Some(catalog);
    config.agent_default_subagent_model = None;
    session.services.models_manager = provider.models_manager(
        config.codex_home.to_path_buf(),
        config.model_catalog.clone(),
    );
    let info = session
        .services
        .models_manager
        .get_model_info("grok-4.6", &config.to_models_manager_config())
        .await;
    turn.provider = provider;
    update_turn_settings_for_test(turn, |settings| {
        settings.model_info = Arc::new(info);
        update_selected_settings_for_test(settings, |selected| {
            selected.collaboration_mode.settings.model = "grok-4.6".into();
            selected.collaboration_mode.settings.reasoning_effort = Some(ReasoningEffort::High);
        });
    });
}

#[test_case::test_case(false, "all", None, None; "v1_full")]
#[test_case::test_case(false, "none", None, None; "v1_fresh")]
#[test_case::test_case(true, "default", None, None; "v2_default")]
#[test_case::test_case(true, "all", None, None; "v2_full")]
#[test_case::test_case(true, "1", Some("grok-4.7"), None; "v2_partial_override")]
#[test_case::test_case(true, "none", Some("grok-4.7"), None; "v2_fresh_override")]
#[test_case::test_case(true, "none", None, Some("grok-4.7"); "configured_child_default")]
#[test_case::test_case(true, "none", Some("grok-4.6"), Some("grok-4.7"); "explicit_child_model_overrides_default")]
#[tokio::test]
async fn grok_spawn_handlers_keep_captured_provider_catalog_and_child_identity(
    v2: bool,
    fork: &str,
    requested: Option<&str>,
    default_model: Option<&str>,
) {
    let (mut session, mut turn) = make_session_and_context().await;
    bind_grok(&mut session, &mut turn).await;
    let mut config = (*turn.config).clone();
    config.agent_default_subagent_model = default_model.map(str::to_owned);
    config
        .features
        .set_enabled(Feature::MultiAgentV2, v2)
        .unwrap();
    set_turn_config(&mut turn, config);
    turn.multi_agent_version = if v2 {
        MultiAgentVersion::V2
    } else {
        MultiAgentVersion::V1
    };
    let parent_provider = turn.provider.info().clone();
    let parent_catalog = turn.config.model_catalog.clone();
    crate::thread_manager::set_thread_manager_test_mode_for_tests(/*enabled*/ true);
    let config = turn.config.as_ref();
    let auth = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy"));
    let manager = ThreadManager::new(
        config,
        auth.clone(),
        crate::thread_manager::build_models_manager(config, auth),
        crate::CodexAppsToolsCache::default(),
        SessionSource::Exec,
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        empty_extension_registry(),
        Arc::new(crate::test_support::EmptyUserInstructionsProvider),
        /*analytics_events_client*/ None,
        crate::thread_manager::passthrough_image_store(),
        thread_store_from_config(config, /*state_db*/ None),
        /*agent_graph_store*/ None,
        "11111111-1111-4111-8111-111111111111".into(),
        /*attestation_provider*/ None,
        /*external_time_provider*/ None,
    );
    let root = manager
        .start_thread(StartThreadOptions {
            history_mode: Some(ThreadHistoryMode::Legacy),
            ..StartThreadOptions::new((*turn.config).clone())
        })
        .await
        .unwrap();
    root.thread.session.new_default_turn().await;
    for index in 0..2 {
        let context = root.thread.session.new_default_turn().await;
        let items = ["user", "assistant"].map(|role| ResponseItem::Message {
            id: None,
            role: role.into(),
            content: vec![if role == "user" {
                ContentItem::InputText {
                    text: format!("grok-seed-{index}"),
                }
            } else {
                ContentItem::OutputText {
                    text: format!("grok-reply-{index}"),
                }
            }],
            phase: (role == "assistant")
                .then_some(codex_protocol::models::MessagePhase::FinalAnswer),
            internal_chat_message_metadata_passthrough: None,
        });
        root.thread
            .session
            .record_conversation_items(context.as_ref(), context.model_info(), &items)
            .await;
    }
    set_agent_control(&mut session, manager.agent_control());
    session.thread_id = root.thread_id;
    let mut args = json!({"message":"inspect this repo"});
    if v2 {
        args["task_name"] = json!("grok_worker");
        if fork != "default" {
            args["fork_turns"] = json!(fork);
        }
    } else {
        args["fork_context"] = json!(fork == "all");
    }
    if let Some(model) = requested {
        args["model"] = json!(model);
    }
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let call = invocation(
        session.clone(),
        turn.clone(),
        "spawn_agent",
        function_payload(args),
    );
    let output = if v2 {
        SpawnAgentHandlerV2::default().handle(call).await
    } else {
        SpawnAgentHandler::default().handle(call).await
    }
    .unwrap();
    let (content, _) = expect_text_output(output);
    let result: serde_json::Value = serde_json::from_str(&content).unwrap();
    let id = if v2 {
        session
            .services
            .local_agent_runtime
            .resolve_agent_reference(
                session.thread_id,
                &turn.session_source,
                result["task_name"].as_str().unwrap(),
            )
            .await
            .unwrap()
    } else {
        parse_agent_id(result["agent_id"].as_str().unwrap())
    };
    let child = manager.get_thread(id).await.unwrap();
    let history = child.session.clone_history().await;
    let history_text = serde_json::to_string(&history.raw_items().collect::<Vec<_>>()).unwrap();
    assert_eq!(
        history_text.contains("grok-seed-0"),
        fork == "all" || fork == "default"
    );
    assert_eq!(history_text.contains("grok-seed-1"), fork != "none");
    let parent_history = root.thread.session.clone_history().await;
    let parent_text =
        serde_json::to_string(&parent_history.raw_items().collect::<Vec<_>>()).unwrap();
    assert!(parent_text.contains("grok-seed-0") && parent_text.contains("grok-seed-1"));
    let snapshot = child.config_snapshot().await;
    let context = child.session.new_default_turn().await;
    assert_eq!(
        (snapshot.model.as_str(), snapshot.model_provider_id.as_str()),
        (
            requested.or(default_model).unwrap_or("grok-4.6"),
            "configured-alias"
        )
    );
    assert_eq!(context.provider.info(), &parent_provider);
    assert_eq!(context.provider.api_dialect(), codex_api::ApiDialect::Grok);
    assert_eq!(context.config.model_catalog, parent_catalog);
    assert!(!context.model_info().used_fallback_model_metadata);
    assert_eq!(
        child
            .session
            .services
            .models_manager
            .get_remote_models()
            .await,
        parent_catalog.as_ref().unwrap().models
    );

    assert_eq!(turn.provider.info(), &parent_provider);
    assert_eq!(turn.config.model.as_deref(), Some("grok-4.6"));
    if v2 {
        child
            .session
            .send_event(
                context.as_ref(),
                EventMsg::TurnComplete(TurnCompleteEvent {
                    turn_id: context.sub_id.clone(),
                    started_at: None,
                    last_agent_message: Some("grok child result".into()),
                    error: None,
                    completed_at: None,
                    duration_ms: None,
                    time_to_first_token_ms: None,
                }),
            )
            .await;
        let expected = format_inter_agent_completion_message(
            AgentPath::root(),
            AgentPath::try_from("/root/grok_worker").unwrap(),
            &AgentStatus::Completed(Some("grok child result".into())),
        )
        .unwrap();
        timeout(Duration::from_secs(5), async {
            loop {
                if manager.captured_ops().iter().any(|(id, op)| *id == root.thread_id && matches!(op,
                    Op::InterAgentCommunication { communication, .. } if communication.content == expected && communication.recipient == AgentPath::root())) { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("actual child completion must reach its parent");
    }
    child.shutdown_and_wait().await.unwrap();
    root.thread.shutdown_and_wait().await.unwrap();
}
