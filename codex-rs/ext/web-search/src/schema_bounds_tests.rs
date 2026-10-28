use codex_extension_api::ToolExecutor;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;

#[test]
fn trusted_web_tool_spec_keeps_unsigned_numeric_bounds() {
    let tool = crate::tool::WebSearchTool {
        session_id: "schema-thread".to_string(),
        http_client_factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        provider: create_model_provider(ModelProviderInfo::default(), /*auth_manager*/ None),
        settings: Default::default(),
        originator: None,
    };
    let spec = serde_json::to_value(tool.spec()).expect("web tool spec");
    let properties = &spec["tools"][0]["parameters"]["properties"];
    for (command, field) in [
        ("click", "id"),
        ("open", "lineno"),
        ("screenshot", "pageno"),
        ("search_query", "recency"),
    ] {
        assert_eq!(
            properties[command]["items"]["properties"][field]["minimum"].as_f64(),
            Some(0.0)
        );
    }
}
