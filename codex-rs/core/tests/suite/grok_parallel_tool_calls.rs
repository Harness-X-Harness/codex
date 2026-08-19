use anyhow::Result;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::configure_grok_test_provider;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_bound_request_enables_parallel_tool_calls() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_grok_mock_server("grok-test").await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("grok-response"),
            responses::ev_completed("grok-response"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_model("grok-test")
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
async fn grok_release_selector_checks_stock_openai_parallel_tool_calls_value() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("openai-response"),
            responses::ev_completed("openai-response"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_model("gpt-5.4")
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn("hello").await?;

    assert_eq!(
        response.single_request().body_json()["parallel_tool_calls"],
        json!(true)
    );
    Ok(())
}
