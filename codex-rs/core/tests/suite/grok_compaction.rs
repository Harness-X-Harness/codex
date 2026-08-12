use super::compact::COMPACT_WARNING_MESSAGE;
use anyhow::Result;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_model_provider_info::WireApi;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_compaction_and_follow_up_keep_the_bound_provider_contract() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("grok-before-compact"),
                responses::ev_assistant_message("grok-before-message", "First response"),
                responses::ev_completed("grok-before-compact"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-compact"),
                responses::ev_assistant_message("grok-summary", "Bound provider summary"),
                responses::ev_completed("grok-compact"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("grok-after-compact"),
                responses::ev_assistant_message("grok-after-message", "Follow-up response"),
                responses::ev_completed("grok-after-compact"),
            ]),
        ],
    )
    .await;

    let test = test_codex()
        .with_model("koffing")
        .with_config(|config| {
            config.model_provider.name = "Grok test provider".to_string();
            config.model_provider.wire_api = WireApi::GrokResponses;
            config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn("Before compact").await?;
    test.codex.submit(Op::Compact).await?;
    let compact_result = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::Warning(warning) if warning.message == COMPACT_WARNING_MESSAGE
        ) || matches!(event, EventMsg::Error(_))
    })
    .await;
    if let EventMsg::Error(error) = compact_result {
        anyhow::bail!("Grok compaction failed: {}", error.message);
    }
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    test.submit_turn("After compact").await?;

    let requests = requests.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        let body = request.body_json();
        assert_eq!(body["model"], "koffing");
        assert!(grok_input_items(&body).iter().all(|item| {
            item.get("type").and_then(Value::as_str) != Some("agent_message")
                && item.get("encrypted_content").is_none()
        }));
    }
    assert!(requests[1].body_contains_text(SUMMARIZATION_PROMPT));
    assert!(requests[2].body_contains_text("Bound provider summary"));
    Ok(())
}

fn grok_input_items(body: &Value) -> &[Value] {
    body["input"]
        .as_array()
        .expect("Grok model input must be an array")
}
