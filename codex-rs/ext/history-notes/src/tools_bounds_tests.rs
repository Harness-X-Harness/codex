use codex_extension_api::ToolExecutor;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;

use super::HistoryNotesAction;
use super::HistoryNotesTool;
use crate::backend::HistoryNotesBackend;

#[test]
fn history_tool_specs_keep_declared_count_and_offset_bounds() {
    for (action, field, minimum) in [
        (HistoryNotesAction::HistoryListWindows, "limit", 1),
        (HistoryNotesAction::HistoryReadItem, "offset_chars", 0),
        (
            HistoryNotesAction::NotesSearchContents,
            "max_matches_per_file",
            1,
        ),
    ] {
        let tool = HistoryNotesTool::new(
            action,
            HistoryNotesBackend::new(
                create_model_provider(ModelProviderInfo::default(), /*auth_manager*/ None),
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            ),
            "thread".to_string(),
            "agent".to_string(),
        );
        let wire = serde_json::to_value(tool.spec()).expect("history spec");
        assert_eq!(
            wire["tools"][0]["parameters"]["properties"][field]["minimum"],
            minimum
        );
    }
}
