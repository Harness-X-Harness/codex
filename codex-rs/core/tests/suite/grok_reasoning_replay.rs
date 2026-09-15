#![allow(clippy::unwrap_used)]

use codex_model_provider_info::WireApi;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_follow_up_omits_reasoning_content_on_encrypted_blob() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let first = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_reasoning_item(
            "rs-1",
            &["look around"],
            &["I am in the workspace and can help."],
        ),
        responses::ev_assistant_message(
            "msg-1",
            "Hi. I am in /tmp/hax. I can help with this project.",
        ),
        responses::ev_completed("resp-1"),
    ]);
    let second = responses::sse(vec![
        responses::ev_response_created("resp-2"),
        responses::ev_assistant_message("msg-2", "你好，需要我做什么？"),
        responses::ev_completed("resp-2"),
    ]);
    let mock = responses::mount_sse_sequence(&server, vec![first, second]).await;

    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.model_provider.name = "xAI".to_string();
        config.model_provider.wire_api = WireApi::GrokResponses;
        config.model_provider.requires_openai_auth = false;
    });
    let test = builder
        .build_with_auto_env(&server)
        .await
        .expect("create Grok Codex conversation");

    test.submit_turn_with_permission_profile("hi", PermissionProfile::read_only())
        .await
        .expect("submit greeting turn");
    test.submit_turn_with_permission_profile("你好", PermissionProfile::read_only())
        .await
        .expect("submit follow-up turn");

    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        2,
        "greeting and follow-up should each POST once"
    );

    let follow_up_reasoning = requests[1]
        .input()
        .into_iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .expect("follow-up must replay the first-turn reasoning item");
    assert_eq!(follow_up_reasoning["id"], "rs-1");
    assert!(
        follow_up_reasoning["encrypted_content"]
            .as_str()
            .is_some_and(|blob| !blob.is_empty()),
        "Grok must keep the first-turn encrypted blob: {follow_up_reasoning}"
    );
    assert!(
        follow_up_reasoning.get("content").is_none(),
        "xAI reports a reasoning content channel as a modified compaction blob: {follow_up_reasoning}"
    );
}
