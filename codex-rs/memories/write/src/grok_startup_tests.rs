//! Actual memory consumers through a configured Grok provider. Tools in the copied
//! test catalog are disabled until C7; this does not certify consolidation tool use.
use super::*;
use codex_model_provider_info::WireApi;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ToolMode;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_memory_phases_use_provider_defaults() -> anyhow::Result<()> {
    grok_memory_phases_use_provider_routes(/*shipped_overrides*/ false).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_memory_phases_use_shipped_overrides() -> anyhow::Result<()> {
    grok_memory_phases_use_provider_routes(/*shipped_overrides*/ true).await
}

async fn grok_memory_phases_use_provider_routes(shipped_overrides: bool) -> anyhow::Result<()> {
    let server = start_mock_server().await;
    let home = Arc::new(TempDir::new()?);
    std::fs::write(
        home.path().join("config.toml"),
        include_str!("../../../../grok/dist/config.toml.example"),
    )?;
    std::fs::write(
        home.path().join("models.json"),
        include_str!("../../../../grok/dist/models.json"),
    )?;
    // Suppress only fixture tool registration, preserving real memory config/model selection.
    let mut profile = std::fs::read_to_string(home.path().join("config.toml"))?;
    profile.push_str(
        r#"
[agents]
enabled = false
[tools.update_plan]
enabled = false
[tools.experimental_request_user_input]
enabled = false
[features]
goals = false
shell_tool = false
view_image = false
sleep_tool = false
multi_agent = false
multi_agent_v2 = false
code_mode = false
apps = false
image_generation = false
tool_suggest = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
"#,
    );
    std::fs::write(home.path().join("config.toml"), profile)?;
    let base_url = format!("{}/grok", server.uri());
    let test = test_codex()
        .with_home(Arc::clone(&home))
        .with_model("grok-4.7")
        .with_config(move |config| {
            config.features.enable(Feature::Sqlite).unwrap();
            let mut provider = config.model_providers["grok"].clone();
            provider.base_url = Some(base_url.clone());
            provider.env_key = None;
            provider.request_max_retries = Some(0);
            provider.stream_max_retries = Some(0);
            config.model_provider = provider;
            config.model_provider_id = "grok".into();
            config.memories.min_rollout_idle_hours = 0;
            config.memories.max_raw_memories_for_consolidation = 1;
            if !shipped_overrides {
                config.memories.extract_model = None;
                config.memories.consolidation_model = None;
            }
            for model in &mut config.model_catalog.as_mut().unwrap().models {
                model.shell_type = ConfigShellToolType::Disabled;
                model.structured_edit_tool_type = None;
                model.node_repl_disabled = true;
                model.tool_mode = Some(ToolMode::Direct);
            }
        })
        .build_with_auto_env(&server)
        .await?;
    assert_eq!(test.config.model_provider.wire_api, WireApi::GrokResponses);
    let provider = create_model_provider(
        test.config.model_provider.clone(),
        Some(test.thread_manager.auth_manager()),
    );
    let (context, config) = memory_startup_context_with_provider(&test, provider).await;
    let db = test.codex.state_db().expect("memory state");
    seed_stage1_candidate(
        db.as_ref(),
        home.path(),
        chrono::Utc::now() - chrono::Duration::hours(2),
        "grok-source",
    )
    .await?;
    let stage_one = json!({"raw_memory":"retained memory", "rollout_summary":"source summary", "rollout_slug":"grok-source"}).to_string();
    let response = core_test_support::responses::mount_sse_sequence(
        &server,
        vec![
            grok_memory_reply("extract", &stage_one),
            grok_memory_reply("consolidate", "consolidated"),
        ],
    )
    .await;
    phase1::run(Arc::clone(&context), Arc::clone(&config)).await;
    let first = wait_for_single_request(&response).await;
    let expected_model = if shipped_overrides {
        "grok-4.7"
    } else {
        "grok-4.6"
    };
    assert_eq!(
        (first.path(), first.body_json()["model"].as_str().unwrap()),
        ("/grok/responses".to_string(), expected_model)
    );
    let store = db.memories_for_version(config.memories.version).await?;
    assert_eq!(
        store.list_stage1_outputs_for_global(/*n*/ 10).await?.len(),
        1
    );
    let root = config
        .codex_home
        .join(config.memories.version.directory_name());
    tokio::fs::create_dir_all(&root).await?;
    seed_extension_instructions(&root).await?;
    seed_required_memory_artifacts(&root).await?;
    let permissions = config.permissions.effective_permission_profile();
    phase2::run(context, config, permissions).await;
    tokio::time::timeout(Duration::from_secs(20), async {
        while response.requests().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    wait_for_phase2_workspace_reset(&store, &root).await?;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let body = request.body_json();
        assert_eq!(
            (request.path(), body["model"].as_str().unwrap()),
            ("/grok/responses".to_string(), expected_model)
        );
        assert!(body.get("store").is_none());
        assert!(body.get("tools").is_none());
        assert!(body.get("client_metadata").is_none());
    }
    shutdown_test_codex(&test).await?;
    Ok(())
}

fn grok_memory_reply(id: &str, text: &str) -> String {
    let mut added = core_test_support::responses::ev_message_item_added(id, "");
    added["output_index"] = json!(0);
    let mut done = ev_assistant_message(id, text);
    done["output_index"] = json!(0);
    sse(vec![
        ev_response_created(id),
        added,
        json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":text}),
        done,
        ev_completed(id),
    ])
}
