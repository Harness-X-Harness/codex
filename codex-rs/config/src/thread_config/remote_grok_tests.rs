use super::*;
use crate::StaticThreadConfigLoader;
use crate::config_toml::ConfigToml;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn remote_loader_rejects_unrepresentable_dialects() {
    for (wire_api, message) in [
        (0, "remote thread config omitted wire_api"),
        (2, "remote thread config returned unknown wire_api: 2"),
        (-1, "remote thread config returned unknown wire_api: -1"),
    ] {
        let mut sources = super::tests::proto_sources();
        let Some(proto::thread_config_source::Source::Session(config)) = &mut sources[0].source
        else {
            panic!("expected session fixture");
        };
        config.model_providers[0].wire_api = wire_api;
        assert_eq!(
            super::tests::load_remote_sources(sources).await,
            Err(ThreadConfigLoadError::new(
                ThreadConfigLoadErrorCode::Parse,
                /*status_code*/ None,
                message,
            ))
        );
    }
}

#[tokio::test]
async fn static_thread_config_preserves_grok_identity_and_configured_policy() {
    let provider = ModelProviderInfo {
        wire_api: WireApi::GrokResponses,
        supports_websockets: false,
        ..super::tests::expected_provider()
    };
    let loader =
        StaticThreadConfigLoader::new(vec![ThreadConfigSource::Session(SessionThreadConfig {
            model_provider: Some("custom".into()),
            model_providers: HashMap::from([("custom".into(), provider.clone())]),
            features: BTreeMap::new(),
        })]);
    let mut layers = loader
        .load_config_layers(ThreadConfigContext::default())
        .await
        .expect("lossless local thread config layers");
    assert_eq!(layers.len(), 1);
    let decoded: ConfigToml = layers.remove(0).config.try_into().expect("provider config");
    assert_eq!(
        (decoded.model_provider, decoded.model_providers),
        (
            Some("custom".into()),
            HashMap::from([("custom".into(), provider)])
        )
    );
}
