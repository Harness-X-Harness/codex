//! Grok-owned proof that the shipped public profile loads through the real
//! Config and Provider path.

use std::path::PathBuf;

use codex_config::config_toml::ConfigToml;
use codex_model_provider_info::WireApi;
use core_test_support::TempDirExt;
use tempfile::tempdir;

use super::Config;
use super::ConfigOverrides;

#[tokio::test]
async fn grok_shipped_public_profile_resolves_to_supported_provider() -> std::io::Result<()> {
    let profile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../grok/dist/config.toml.example");
    let contents = std::fs::read_to_string(&profile)?;
    let cfg: ConfigToml =
        toml::from_str(&contents).expect("shipped public Grok profile should parse");

    let codex_home = tempdir()?;
    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.abs(),
    )
    .await?;

    assert_eq!(config.model.as_deref(), Some("grok-4.6"));
    assert_eq!(config.model_provider_id, "grok");
    assert_eq!(config.model_catalog, None);
    assert_eq!(config.agent_default_subagent_model, None);

    let provider = &config.model_provider;
    assert_eq!(
        provider.base_url.as_deref(),
        Some("https://grok.trustedtunnel.app/v1")
    );
    assert_eq!(provider.wire_api, WireApi::GrokResponses);
    assert!(!provider.requires_openai_auth);
    assert!(!provider.supports_websockets);
    assert_eq!(provider.env_key.as_deref(), Some("GROK_API_KEY"));
    assert!(provider.experimental_bearer_token.is_none());
    Ok(())
}
