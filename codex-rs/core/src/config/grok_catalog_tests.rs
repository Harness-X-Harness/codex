use super::ConfigBuilder;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::RefreshStrategy;
use codex_prompts::render_model_instructions;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test_case::test_case(false; "default_profile")]
#[test_case::test_case(true; "selected_profile_v2")]
#[tokio::test]
async fn shipped_profile_resolves_relative_catalog_and_preserves_typed_policy(
    named: bool,
) -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let cwd = TempDir::new()?;
    let profile = codex_utils_cargo_bin::find_resource!("../../grok/dist/config.toml.example")?;
    let catalog = codex_utils_cargo_bin::find_resource!("../../grok/dist/models.json")?;
    let selected_path = home.path().join(if named {
        "work.config.toml"
    } else {
        "config.toml"
    });
    std::fs::copy(profile, &selected_path)?;
    if named {
        std::fs::write(
            home.path().join("config.toml"),
            "model_provider = \"openai\"\n",
        )?;
    }
    let bytes = std::fs::read(catalog)?;
    std::fs::write(home.path().join("models.json"), &bytes)?;
    let mut loader_overrides = codex_config::LoaderOverrides::without_managed_config_for_tests();
    if named {
        loader_overrides.user_config_path =
            Some(codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(selected_path)?);
        loader_overrides.user_config_profile = Some("work".parse()?);
    }
    let config = ConfigBuilder::without_managed_config_for_tests()
        .loader_overrides(loader_overrides)
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(cwd.path().to_path_buf()))
        .build()
        .await?;
    assert_eq!(
        (
            config.model.as_deref(),
            config.model_provider_id.as_str(),
            config.model_provider.wire_api,
            config.model_provider.env_key.as_deref(),
            config.model_provider.requires_openai_auth,
            config.model_provider.supports_websockets,
            config.memories.extract_model.as_deref(),
            config.memories.consolidation_model.as_deref(),
            config.agent_default_subagent_model.as_deref()
        ),
        (
            Some("grok-4.7"),
            "grok",
            WireApi::GrokResponses,
            Some("GROK_API_KEY"),
            false,
            false,
            Some("grok-4.7"),
            Some("grok-4.7"),
            None
        )
    );
    // The accepted catalog's legacy empty template was already promoted by ModelsResponse.
    // Loading individual ModelInfo values skips that compatibility decoder; state the
    // intended promotion explicitly so this test catches a changed effective prompt.
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut expected: Vec<ModelInfo> = serde_json::from_value(value["models"].clone())?;
    for model in &mut expected {
        model.model_messages = Some(ModelMessages {
            instructions_template: Some(String::new()),
            ..Default::default()
        });
    }
    assert_eq!(
        config.model_catalog,
        Some(ModelsResponse {
            models: expected.clone()
        })
    );
    let provider = create_model_provider(config.model_provider.clone(), /*auth_manager*/ None);
    let manager = provider.models_manager(
        config.codex_home.to_path_buf(),
        config.model_catalog.clone(),
    );
    let presets = manager
        .list_models(RefreshStrategy::Online, config.http_client_factory())
        .await;
    assert_eq!(
        presets
            .iter()
            .map(|model| (model.model.as_str(), model.is_default))
            .collect::<Vec<_>>(),
        vec![("grok-4.7", true), ("grok-4.6", false)]
    );
    for expected_model in expected {
        let model = manager
            .get_model_info(&expected_model.slug, &config.to_models_manager_config())
            .await;
        assert_eq!(model, expected_model);
        assert_eq!(render_model_instructions(&model), "");
        assert_eq!(model.usable_context_window(), Some(475_000));
        assert_eq!(model.auto_compact_token_limit(), Some(400_000));
    }
    Ok(())
}

#[tokio::test]
async fn shipped_profile_rejects_missing_malformed_and_empty_catalog() -> anyhow::Result<()> {
    for contents in [None, Some("not json"), Some(r#"{"models":[]}"#)] {
        let home = TempDir::new()?;
        std::fs::copy(
            codex_utils_cargo_bin::find_resource!("../../grok/dist/config.toml.example")?,
            home.path().join("config.toml"),
        )?;
        if let Some(contents) = contents {
            std::fs::write(home.path().join("models.json"), contents)?;
        }
        let error = ConfigBuilder::without_managed_config_for_tests()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .build()
            .await
            .expect_err("an invalid explicit catalog must never use a fallback");
        assert_eq!(
            error.kind(),
            if contents.is_none() {
                std::io::ErrorKind::NotFound
            } else {
                std::io::ErrorKind::InvalidData
            }
        );
    }
    Ok(())
}
