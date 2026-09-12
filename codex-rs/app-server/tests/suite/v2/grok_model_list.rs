use super::*;
use codex_app_server_protocol::MultiAgentVersion;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

/// A Grok-bound App Server process serves exactly the release-bundled Grok
/// catalog through the stock `Model` DTO: one process, one Provider, one
/// catalog. Nothing from another Provider is merged in.
#[tokio::test]
async fn list_models_uses_grok_release_catalog_through_stock_model_dto() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"model = "grok-4.6"
model_provider = "grok"

[model_providers.grok]
name = "Grok"
base_url = "https://grok.com/api/codex/v1"
wire_api = "grok_responses"
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
        panic!("expected the single release-bundled Grok model, got {data:?}");
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
