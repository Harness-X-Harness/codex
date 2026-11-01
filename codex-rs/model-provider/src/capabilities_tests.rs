use super::*;
use crate::create_model_provider;
use codex_model_provider_info::ModelProviderCapabilities;
use pretty_assertions::assert_eq;

#[test]
fn capability_overrides_preserve_unspecified_provider_defaults() {
    for info in [
        ModelProviderInfo::default(),
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        ModelProviderInfo {
            name: "Azure".to_string(),
            ..Default::default()
        },
    ] {
        let defaults = create_model_provider(info.clone(), /*auth_manager*/ None).capabilities();
        let provider = create_model_provider(
            ModelProviderInfo {
                capabilities: Some(ModelProviderCapabilities {
                    external_web_access: Some(false),
                    ..Default::default()
                }),
                ..info
            },
            /*auth_manager*/ None,
        );
        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                external_web_access: false,
                ..defaults
            },
        );
    }
}

#[test]
fn explicit_compaction_override_stays_within_dialect_support() {
    for (wire_api, remote_compaction) in [
        (WireApi::Responses, RemoteCompactionSupport::V2),
        (WireApi::GrokResponses, RemoteCompactionSupport::Unsupported),
    ] {
        let provider = create_model_provider(
            ModelProviderInfo {
                wire_api,
                capabilities: Some(ModelProviderCapabilities {
                    external_web_access: Some(false),
                    remote_compaction: Some(RemoteCompactionSupport::V2),
                }),
                ..Default::default()
            },
            /*auth_manager*/ None,
        );
        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                external_web_access: false,
                remote_compaction,
                ..ProviderCapabilities::default()
            }
        );
    }
}
