use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;

use super::image_generation_available;

#[test]
fn grok_wire_enables_existing_image_extension() {
    let grok = ModelProviderInfo {
        wire_api: WireApi::GrokResponses,
        ..ModelProviderInfo::default()
    };

    assert!(image_generation_available(&grok));
}
