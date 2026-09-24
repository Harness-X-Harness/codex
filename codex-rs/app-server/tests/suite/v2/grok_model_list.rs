use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::MultiAgentVersion;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

/// A Grok-bound App Server process without `model_catalog_json` serves the
/// Rust compatibility fallback through the stock `Model` DTO: one process,
/// one Provider, one catalog. Nothing from another Provider is merged in.
#[tokio::test]
async fn list_models_uses_grok_compatibility_fallback_through_stock_model_dto() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"model = "grok-4.6"
model_provider = "grok"

[model_providers.grok]
name = "Grok"
base_url = "https://grok.com/api/codex/v1"
wire_api = "grok_responses"
requires_openai_auth = false
supports_websockets = false
"#,
    )?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let ModelListResponse { data, next_cursor } = mcp
        .request(|request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams {
                limit: Some(100),
                cursor: None,
                include_hidden: None,
            },
        })
        .await?;

    let [model] = data.as_slice() else {
        panic!("expected the single Grok compatibility fallback model, got {data:?}");
    };
    assert_eq!(model.id, "grok-4.6");
    assert_eq!(model.model, "grok-4.6");
    assert_eq!(model.display_name, "Grok 4.6");
    assert_eq!(model.default_reasoning_effort, ReasoningEffort::High);
    assert_eq!(
        model
            .supported_reasoning_efforts
            .iter()
            .map(|option| option.reasoning_effort.clone())
            .collect::<Vec<_>>(),
        vec![
            ReasoningEffort::Ultra,
            ReasoningEffort::XHigh,
            ReasoningEffort::High,
            ReasoningEffort::Medium,
            ReasoningEffort::Low,
        ]
    );
    assert_eq!(model.multi_agent_version, Some(MultiAgentVersion::V2));
    assert!(model.is_default);
    assert!(next_cursor.is_none());
    Ok(())
}

/// The shipped `grok/dist` profile loads `models.json` through the stock
/// relative `model_catalog_json` seam. `grok-4.7` is the picker default;
/// `grok-4.6` stays selectable. `grok-build` is not a request slug.
#[tokio::test]
async fn list_models_uses_shipped_catalog_json() -> Result<()> {
    let codex_home = TempDir::new()?;
    let profile = codex_utils_cargo_bin::find_resource!("../../grok/dist/config.toml.example")?;
    let catalog = codex_utils_cargo_bin::find_resource!("../../grok/dist/models.json")?;
    std::fs::copy(&profile, codex_home.path().join("config.toml"))?;
    std::fs::copy(&catalog, codex_home.path().join("models.json"))?;
    let catalog_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&catalog)?)?;
    let models = catalog_json["models"]
        .as_array()
        .expect("shipped catalog models");
    assert_eq!(
        models
            .iter()
            .map(|model| model["context_window"].as_i64())
            .collect::<Vec<_>>(),
        vec![Some(500_000), Some(500_000)]
    );
    assert_eq!(
        models
            .iter()
            .map(|model| {
                (
                    model["slug"].as_str().unwrap_or_default(),
                    model["auto_review_model_override"]
                        .as_str()
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>(),
        vec![("grok-4.7", "grok-4.7"), ("grok-4.6", "grok-4.6")]
    );

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let ModelListResponse { data, next_cursor } = mcp
        .request(|request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams {
                limit: Some(100),
                cursor: None,
                include_hidden: None,
            },
        })
        .await?;

    assert_eq!(
        data,
        vec![
            shipped_grok_model("grok-4.7", "Grok 4.7", /*is_default*/ true),
            shipped_grok_model("grok-4.6", "Grok 4.6", /*is_default*/ false),
        ]
    );
    assert!(next_cursor.is_none());
    Ok(())
}

fn shipped_grok_model(id: &str, display_name: &str, is_default: bool) -> Model {
    Model {
        id: id.to_string(),
        model: id.to_string(),
        upgrade: None,
        upgrade_info: None,
        availability_nux: None,
        display_name: display_name.to_string(),
        description: format!("{display_name} model"),
        model_specialty: None,
        hidden: false,
        supported_reasoning_efforts: vec![
            reasoning_option(ReasoningEffort::Ultra, "Ultra reasoning"),
            reasoning_option(ReasoningEffort::XHigh, "Maximum reasoning"),
            reasoning_option(ReasoningEffort::High, "High reasoning"),
            reasoning_option(ReasoningEffort::Medium, "Medium reasoning"),
            reasoning_option(ReasoningEffort::Low, "Low reasoning"),
        ],
        default_reasoning_effort: ReasoningEffort::High,
        input_modalities: vec![InputModality::Text, InputModality::Image],
        supports_personality: false,
        multi_agent_version: Some(MultiAgentVersion::V2),
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        available_access_programs: None,
        is_default,
    }
}

fn reasoning_option(effort: ReasoningEffort, description: &str) -> ReasoningEffortOption {
    ReasoningEffortOption {
        reasoning_effort: effort,
        description: description.to_string(),
    }
}
