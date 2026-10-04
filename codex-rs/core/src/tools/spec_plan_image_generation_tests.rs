use super::*;
use codex_model_provider::ModelProvider;
use codex_model_provider::ModelProviderFuture;
use codex_model_provider::ProviderAccountResult;
use codex_model_provider::ProviderCapabilities;
use codex_model_provider::SharedModelProvider;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn image_inputs() -> ToolPlanInputs {
    ToolPlanInputs {
        extension_tool_executors: vec![Arc::new(TestNamespaceExtensionTool {
            namespace: "image_gen",
            tool_name: "imagegen",
        })],
        ..Default::default()
    }
}

fn use_image_provider(turn: &mut TurnContext, name: &str, wire_api: WireApi) {
    let provider = ModelProviderInfo {
        name: name.to_string(),
        base_url: Some("https://api.x.ai/v1".to_string()),
        wire_api,
        ..Default::default()
    };
    update_config(turn, |config| config.model_provider = provider.clone());
    turn.provider = create_model_provider(provider, /*auth_manager*/ None);
    turn.auth_manager = None;
    set_feature(turn, Feature::ImageGeneration, /*enabled*/ true);
    update_turn_settings_for_test(turn, |settings| {
        Arc::make_mut(&mut settings.model_info).input_modalities = vec![InputModality::Image];
    });
}

#[tokio::test]
async fn image_visibility_uses_explicit_dialect_for_aliases_and_code_mode() {
    for name in ["Grok", "OpenAI", "Custom endpoint"] {
        for wire_api in [WireApi::Responses, WireApi::GrokResponses] {
            for code_mode in [false, true] {
                let plan = probe_with(
                    |turn| {
                        use_image_provider(turn, name, wire_api);
                        if code_mode {
                            set_features(turn, &[Feature::CodeMode, Feature::CodeModeOnly]);
                        }
                    },
                    image_inputs(),
                )
                .await;
                let tool = ToolName::namespaced("image_gen", "imagegen");
                let present = plan.registered_names.contains(&tool.to_string());
                assert_eq!(present, wire_api == WireApi::GrokResponses);
                if code_mode {
                    assert_eq!(
                        plan.code_mode_tool_names.values().any(|name| name == &tool),
                        present
                    );
                } else {
                    assert_eq!(
                        plan.visible_names.iter().any(|name| name == "image_gen"),
                        present
                    );
                }
            }
        }
    }
}

#[derive(Debug)]
struct CappedImageProvider {
    inner: SharedModelProvider,
    capabilities: ProviderCapabilities,
}

impl ModelProvider for CappedImageProvider {
    fn info(&self) -> &ModelProviderInfo {
        self.inner.info()
    }
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }
    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.inner.auth_manager()
    }
    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        self.inner.auth()
    }
    fn account_state(&self) -> ProviderAccountResult {
        self.inner.account_state()
    }
    fn models_manager(
        &self,
        codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.inner.models_manager(codex_home, config_model_catalog)
    }
}

#[tokio::test]
async fn grok_image_visibility_preserves_feature_modality_capability_and_free_plan_gates() {
    for gate in ["feature", "modality", "images", "namespaces", "free"] {
        let plan = probe_with(
            |turn| {
                use_image_provider(turn, "Custom endpoint", WireApi::GrokResponses);
                match gate {
                    "feature" => {
                        set_feature(turn, Feature::ImageGeneration, /*enabled*/ false)
                    }
                    "modality" => update_turn_settings_for_test(turn, |settings| {
                        Arc::make_mut(&mut settings.model_info).input_modalities =
                            vec![InputModality::Text];
                    }),
                    "images" | "namespaces" => {
                        turn.provider = Arc::new(CappedImageProvider {
                            inner: turn.provider.clone(),
                            capabilities: ProviderCapabilities {
                                image_generation: gate != "images",
                                namespace_tools: gate != "namespaces",
                                ..Default::default()
                            },
                        });
                    }
                    "free" => {
                        let auth = CodexAuth::from_external_chatgpt_tokens(
                            "e30.e30.sig",
                            "test-account",
                            Some("free"),
                        )
                        .expect("free account fixture");
                        turn.auth_manager = Some(AuthManager::from_auth_for_testing(auth));
                    }
                    _ => unreachable!(),
                }
            },
            image_inputs(),
        )
        .await;
        plan.assert_visible_lacks(&["image_gen"]);
        plan.assert_registered_lacks(&[&ToolName::namespaced("image_gen", "imagegen").to_string()]);
    }
}
