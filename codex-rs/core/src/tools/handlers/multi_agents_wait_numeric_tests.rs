use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn v2_signed_numeric_wait_keeps_configured_minimum_and_explanation() {
    let (session, mut turn) = make_session_and_context().await;
    let mut config = (*turn.config).clone();
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    config.multi_agent_v2.min_wait_timeout_ms = 50;
    config.multi_agent_v2.max_wait_timeout_ms = 1_000;
    config.multi_agent_v2.default_wait_timeout_ms = 50;
    set_turn_config(&mut turn, config);
    let session = Arc::new(session);
    let turn = Arc::new(turn);

    for (token, requested) in [("-1.0", -1), ("-1e0", -1), ("-0.0", 0)] {
        tokio::time::pause();
        let started_at = tokio::time::Instant::now();
        let output = WaitAgentHandlerV2::default()
            .handle(invocation(
                session.clone(),
                turn.clone(),
                "wait_agent",
                ToolPayload::Function {
                    arguments: format!(r#"{{"timeout_ms":{token}}}"#),
                },
            ))
            .await
            .expect("signed whole timeout should use v2 policy");
        let elapsed = started_at.elapsed();
        tokio::time::resume();

        assert!(
            elapsed >= Duration::from_millis(50) && elapsed <= Duration::from_millis(51),
            "configured minimum must remain effective: {elapsed:?}"
        );
        let (content, success) = expect_text_output(output);
        let result: crate::tools::handlers::multi_agents_v2::wait::WaitAgentResult =
            serde_json::from_str(&content).expect("wait result should be json");
        assert_eq!(
            result,
            crate::tools::handlers::multi_agents_v2::wait::WaitAgentResult {
                message: format!(
                    "Wait timed out.\n\nRequested timeout of {requested}ms was clamped to the minimum of 50ms."
                ),
                timed_out: true,
            }
        );
        assert_eq!(success, None);
    }
}
