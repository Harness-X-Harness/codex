use super::*;
use pretty_assertions::assert_eq;

fn write_literal_wait_pre_tool_use_hook(home: &Path, timeout_literal: &str) -> Result<()> {
    let script_path = home.join("pre_tool_use_hook.py");
    let log_path = home.join("pre_tool_use_hook_log.jsonl");
    let output = format!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{{"timeout_ms":{timeout_literal}}}}}}}"#
    );
    // Quote the entire response as a string: Python must never parse or round its number.
    let output_string = serde_json::to_string(&output)?;
    let script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

sys.stdout.write({output_string})
"#,
        log_path = log_path.display(),
    );
    let hooks = json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "^wait_agent$",
                "hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script_path.display()),
                    "statusMessage": "rewriting wait timeout",
                }]
            }]
        }
    });

    fs::write(script_path, script).context("write literal wait hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

#[tokio::test]
async fn pre_tool_use_rewrites_wait_agent_exact_numeric_timeout() -> Result<()> {
    skip_if_no_network!(Ok(()));

    for (timeout_literal, error_message) in [
        ("1.0", None),
        ("1e0", None),
        (
            "1.0000000000000001",
            Some("failed to parse function arguments: expected an in-range whole number"),
        ),
    ] {
        let server = start_mock_server().await;
        let call_id = "pretooluse-wait-numeric-rewrite";
        // If the hook is ignored, this fails differently from numeric admission.
        let original_args = json!({"timeout_ms": "original-invalid-timeout"});
        let responses = mount_sse_sequence(
            &server,
            vec![
                sse(vec![
                    ev_response_created("resp-1"),
                    ev_function_call(
                        call_id,
                        "wait_agent",
                        &serde_json::to_string(&original_args)?,
                    ),
                    ev_completed("resp-1"),
                ]),
                sse(vec![
                    ev_response_created("resp-2"),
                    ev_assistant_message("msg-1", "wait result received"),
                    ev_completed("resp-2"),
                ]),
            ],
        )
        .await;
        let mut builder = test_codex()
            .with_model("test-gpt-5.1-codex")
            .with_pre_build_hook(move |home| {
                write_literal_wait_pre_tool_use_hook(home, timeout_literal)
                    .expect("write literal numeric hook fixture");
            })
            .with_config(|config| {
                trust_discovered_hooks(config);
                config
                    .features
                    .enable(Feature::MultiAgentV2)
                    .expect("enable multi-agent v2");
                config.multi_agent_v2.min_wait_timeout_ms = 0;
                config.multi_agent_v2.max_wait_timeout_ms = 1;
                config.multi_agent_v2.default_wait_timeout_ms = 0;
            });
        let test = builder.build_with_auto_env(&server).await?;
        test.submit_turn("wait using the rewritten timeout").await?;

        let requests = responses.requests();
        assert_eq!(requests.len(), 2, "timeout literal {timeout_literal}");
        let output_item = requests[1].function_call_output(call_id);
        let output = output_item["output"]
            .as_str()
            .expect("wait_agent function output string");
        match error_message {
            Some(message) => assert!(
                output.contains(message),
                "timeout literal {timeout_literal}: {output}"
            ),
            None => assert_eq!(
                serde_json::from_str::<Value>(output)?,
                json!({"message": "Wait timed out.", "timed_out": true}),
                "timeout literal {timeout_literal}"
            ),
        }

        let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
        assert_eq!(hook_inputs.len(), 1, "timeout literal {timeout_literal}");
        assert_eq!(
            json!({
                "hook_event_name": hook_inputs[0]["hook_event_name"],
                "tool_name": hook_inputs[0]["tool_name"],
                "tool_use_id": hook_inputs[0]["tool_use_id"],
                "tool_input": hook_inputs[0]["tool_input"],
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "wait_agent",
                "tool_use_id": call_id,
                "tool_input": original_args,
            }),
            "timeout literal {timeout_literal}"
        );
    }

    Ok(())
}
