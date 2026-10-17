use super::*;
use crate::context_manager::ContextManager;
use codex_history::ResponseItemEnvelope;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::openai_models::InputModality;
use pretty_assertions::assert_eq;

#[test]
fn retired_hosted_x_cannot_become_a_synthetic_local_pair_during_request_building()
-> anyhow::Result<()> {
    let mut client = test_model_client(SessionSource::Cli);
    let provider =
        create_oss_provider_with_base_url("https://example.com/v1", WireApi::GrokResponses);
    Arc::get_mut(&mut client.state)
        .expect("unique test client")
        .provider = create_model_provider(provider, /*auth_manager*/ None);
    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        for has_local_output in [false, true] {
            let mut saved = vec![serde_json::from_value::<ResponseItem>(json!({
                "type": "custom_tool_call", "id": "legacy-provider-id",
                "call_id": "legacy-call", "name": name,
                "status": "completed", "input": "original opaque input",
            }))?];
            if has_local_output {
                saved.push(serde_json::from_value(json!({
                    "type": "custom_tool_call_output", "call_id": "legacy-call",
                    "name": name, "output": "genuine saved local result",
                }))?);
            }
            let mut history = ContextManager::new();
            history.replace_annotated(
                saved
                    .iter()
                    .cloned()
                    .map(ResponseItemEnvelope::new)
                    .collect(),
            );
            let normalized = history.clone().for_prompt_except_hosted(
                &[InputModality::Text],
                codex_protocol::grok_hosted::is_completed_search,
            );
            assert_eq!(normalized, saved);
            let prompt = Prompt {
                input: normalized,
                ..Default::default()
            };
            let result = client.build_responses_request(
                &prompt,
                &test_model_info(),
                /*effort*/ None,
                codex_protocol::config_types::ReasoningSummary::None,
                /*service_tier*/ None,
                &test_responses_metadata_for_client(
                    &client,
                    /*turn_id*/ None,
                    format!("{}:0", client.state.thread_id),
                    /*parent_thread_id*/ None,
                    TestCodexResponsesRequestKind::Turn,
                ),
                /*include_internal*/ true,
            );
            if has_local_output {
                let request = result?;
                assert_eq!(request.input.len(), 2);
                assert!(matches!(
                    request.input[0],
                    ResponseItem::FunctionCall { .. }
                ));
                assert!(matches!(
                    request.input[1],
                    ResponseItem::FunctionCallOutput { .. }
                ));
                assert_eq!(
                    serde_json::to_value(&request.input[1])?["output"],
                    "genuine saved local result"
                );
            } else {
                let error = result.expect_err("retired Hosted X Search history must be rejected");
                assert!(matches!(
                    error.details(),
                    CodexErrorDetails::InvalidRequest(message)
                        if message == "unsupported history: retired Hosted X Search cannot be replayed"
                ));
            }
            assert_eq!(history.raw_items().cloned().collect::<Vec<_>>(), saved);
            assert_eq!(prompt.input, saved);
        }
    }
    Ok(())
}
