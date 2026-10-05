use super::*;
use codex_model_provider_info::WireApi;

#[test_case::test_case(Some("grok-4.7"); "shipped_primary")]
#[test_case::test_case(Some("grok-4.6"); "shipped_pinned")]
#[test_case::test_case(None; "provider_fallback")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_grok_reviewer_uses_catalog_override_or_provider_default(
    model: Option<&str>,
) -> anyhow::Result<()> {
    let server = start_mock_server().await;
    let assessment = r#"{"outcome":"allow"}"#;
    let mut added = core_test_support::responses::ev_message_item_added("review", "");
    added["output_index"] = serde_json::json!(0);
    let mut done = ev_assistant_message("review", assessment);
    done["output_index"] = serde_json::json!(0);
    let requests = mount_sse_once(&server, sse(vec![ev_response_created("review"), added,
        serde_json::json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":assessment}),
        done, ev_completed("review")])).await;
    let (mut session, mut turn) = guardian_test_session_and_turn(&server).await;
    let mut config = (*turn.config).clone();
    config.model_provider_id = "alias".into();
    config.model_provider = ModelProviderInfo {
        name: "arbitrary alias".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api: WireApi::GrokResponses,
        ..Default::default()
    };
    // Fixture restrictions only: reviewer execution/inspection tools await C7 wire support.
    for feature in [
        Feature::ShellTool,
        Feature::UnifiedExec,
        Feature::ViewImage,
        Feature::CodeMode,
    ] {
        config.features.disable(feature)?;
    }
    config.model_catalog = if model.is_some() {
        Some(serde_json::from_slice(&std::fs::read(
            codex_utils_cargo_bin::find_resource!("../../grok/dist/models.json")?,
        )?)?)
    } else {
        None
    };
    let provider = create_model_provider(config.model_provider.clone(), /*auth_manager*/ None);
    let manager = provider.models_manager(
        config.codex_home.to_path_buf(),
        config.model_catalog.clone(),
    );
    let parent_model = model.unwrap_or("unlisted-parent");
    let model_info = manager
        .get_model_info(parent_model, &config.to_models_manager_config())
        .await;
    Arc::get_mut(&mut session).unwrap().services.models_manager = Arc::clone(&manager);
    crate::guardian::test_host::install(&session, &config);
    let turn_mut = Arc::get_mut(&mut turn).unwrap();
    turn_mut.config = Arc::new(config);
    turn_mut.provider = provider;
    update_turn_settings_for_test(turn_mut, |settings| {
        settings.model_info = Arc::new(model_info);
    });
    let (outcome, analytics) = run_guardian_review_session_for_test(
        session,
        turn,
        guardian_exec_command_request("grok-review"),
        ApprovalRequestReasons::default(),
        /*external_cancel*/ None,
        /*max_attempts*/ 1,
    )
    .await;
    assert!(matches!(outcome, GuardianReviewOutcome::Completed(_)));
    let request = requests.single_request();
    let body = request.body_json();
    assert_eq!(body["model"], model.unwrap_or("grok-4.6"));
    assert_eq!(request.path(), "/v1/responses");
    assert!(body.get("store").is_none());
    assert!(body.get("tools").is_none());
    assert_eq!(
        analytics.guardian_default_review_model_id.as_deref(),
        Some("grok-4.6")
    );
    assert_eq!(
        analytics.guardian_review_model_overridden,
        Some(model.is_some())
    );
    Ok(())
}
