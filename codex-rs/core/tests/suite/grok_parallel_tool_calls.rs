use anyhow::Result;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::configure_grok_test_provider;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_request_enables_parallel_tool_calls() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_grok_mock_server("koffing").await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_completed("response-1"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_model("koffing")
        .with_config(configure_grok_test_provider)
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn("hello").await?;

    assert_eq!(
        response.single_request().body_json()["parallel_tool_calls"],
        json!(true)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stock_openai_request_preserves_parallel_tool_calls_capability() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_completed("response-1"),
        ]),
    )
    .await;
    let test = test_codex().build_with_auto_env(&server).await?;

    test.submit_turn("hello").await?;

    assert_eq!(
        response.single_request().body_json()["parallel_tool_calls"],
        json!(true)
    );
    Ok(())
}
