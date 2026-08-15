use super::*;
use codex_model_provider_info::WireApi;
use codex_model_provider_info::create_oss_provider_with_base_url;

#[test]
fn request_strategy_changes_with_effective_route() {
    let provider = |base_url| {
        create_oss_provider_with_base_url(base_url, WireApi::Responses)
            .to_api_provider(/*auth_mode*/ None)
            .expect("test provider should resolve")
    };
    let first = ProviderRequestSetup::new(
        None,
        provider("https://first.example/v1"),
        crate::unauthenticated_auth_provider(),
        None,
    );
    let second = ProviderRequestSetup::new(
        None,
        provider("https://second.example/v1"),
        crate::unauthenticated_auth_provider(),
        None,
    );

    assert_ne!(first.strategy, second.strategy);
}

#[test]
fn request_strategy_changes_with_effective_auth_identity() {
    let provider_info =
        create_oss_provider_with_base_url("https://provider.example/v1", WireApi::Responses);
    let api_provider = provider_info
        .to_api_provider(/*auth_mode*/ None)
        .expect("test provider should resolve");
    let first_auth = CodexAuth::from_api_key("first-key");
    let second_auth = CodexAuth::from_api_key("second-key");
    let first = ProviderRequestSetup::new(
        Some(first_auth.clone()),
        api_provider.clone(),
        resolve_provider_auth(Some(&first_auth), &provider_info)
            .expect("first auth should resolve"),
        None,
    );
    let second = ProviderRequestSetup::new(
        Some(second_auth.clone()),
        api_provider,
        resolve_provider_auth(Some(&second_auth), &provider_info)
            .expect("second auth should resolve"),
        None,
    );

    assert_ne!(first.strategy, second.strategy);
}
