use anyhow::Result;
use codex_core::compact::SUMMARY_PREFIX;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;
use test_case::test_case;
use wiremock::MockServer;

const MODEL: &str = "grok-compaction-fixture";
const COMPACT_PROMPT: &str = "Summarize the conversation for the next turn.";
const SUMMARY: &str = "The retained summary is compaction-result-731.";

#[derive(Clone, Copy)]
enum CompactionMode {
    Manual,
    Automatic,
}

fn grok_reply(id: &str, text: &str, total_tokens: i64) -> String {
    let mut added = responses::ev_message_item_added(id, "");
    added["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message(id, text);
    done["output_index"] = json!(0);
    responses::sse(vec![
        responses::ev_response_created(id),
        added,
        json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": text
        }),
        done,
        responses::ev_completed_with_tokens(id, total_tokens),
    ])
}

#[test_case("Grok", CompactionMode::Manual; "grok_manual")]
#[test_case("OpenAI", CompactionMode::Manual; "openai_alias_manual")]
#[test_case("Azure", CompactionMode::Manual; "azure_alias_manual")]
#[test_case("Grok", CompactionMode::Automatic; "grok_automatic")]
#[test_case("OpenAI", CompactionMode::Automatic; "openai_alias_automatic")]
#[test_case("Azure", CompactionMode::Automatic; "azure_alias_automatic")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_compaction_keeps_local_summary_across_provider_aliases(
    provider_name: &str,
    mode: CompactionMode,
) -> Result<()> {
    let server = MockServer::start().await;
    let home = Arc::new(TempDir::new()?);
    let catalog = json!({"models": [{
        "slug": MODEL,
        "display_name": MODEL,
        "supported_reasoning_levels": [],
        "shell_type": "disabled",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "model_messages": {"instructions_template": "Answer the user directly."},
        "support_verbosity": false,
        "apply_patch_tool_type": null,
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "experimental_supported_tools": [],
        "context_window": 1000000,
        "supports_reasoning_summary_parameter": false,
        "include_apps_usage_instructions": false,
        "node_repl_disabled": true,
        "tool_mode": "direct"
    }]});
    std::fs::write(home.path().join("models.json"), serde_json::to_vec(&catalog)?)?;
    let base_url = format!("{}/v1", server.uri());
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model = "{MODEL}"
model_provider = "grok"
model_catalog_json = "models.json"
model_auto_compact_token_limit = 100000
compact_prompt = "{COMPACT_PROMPT}"
approval_policy = "never"
sandbox_mode = "read-only"
web_search = "disabled"
[model_providers.grok]
name = "{provider_name}"
base_url = "{base_url}"
wire_api = "grok_responses"
requires_openai_auth = false
supports_websockets = false
[tools.update_plan]
enabled = false
[tools.experimental_request_user_input]
enabled = false
[features]
goals = false
shell_tool = false
view_image = false
sleep_tool = false
multi_agent = false
multi_agent_v2 = false
code_mode = false
apps = false
image_generation = false
tool_suggest = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
"#
        ),
    )?;
    let initial_tokens = match mode {
        CompactionMode::Manual => 2_000,
        CompactionMode::Automatic => 200_000,
    };
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            grok_reply("before", "The first answer.", initial_tokens),
            grok_reply("summary", SUMMARY, /*total_tokens*/ 200),
            grok_reply("after", "The final answer.", /*total_tokens*/ 120),
        ],
    )
    .await;
    let test = test_codex()
        .with_home(home)
        .with_model(MODEL)
        .with_config(|config| {
            // The generic builder replaces the transport after loading the profile.
            assert_eq!(config.model_provider_id, "grok");
            config.model_provider = config.model_providers["grok"].clone();
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_text_turn("Remember the first user message.")
        .await?;
    assert_eq!(requests.requests().len(), 1);
    match mode {
        CompactionMode::Manual => {
            test.codex.submit(Op::Compact).await?;
            wait_for_event(&test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await;
            // Prove the manual request completed before the continuation can run.
            assert_eq!(requests.requests().len(), 2);
        }
        CompactionMode::Automatic => {}
    }
    test.submit_text_turn("Continue after compaction.").await?;

    let requests = requests.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        let body = request.body_json();
        assert_eq!(body["model"], MODEL);
        assert_eq!(request.path(), "/v1/responses");
        assert!(body.get("tools").is_none());
        assert!(body.get("store").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
        assert!(
            request
                .input()
                .iter()
                .all(|item| item["type"] != "compaction_trigger")
        );
    }
    assert!(!requests[0].body_contains_text(SUMMARY));
    assert!(!requests[1].body_contains_text(SUMMARY));
    // Stock assigns fresh item IDs while recording and installing compacted history.
    let compact_input = requests[1]
        .input()
        .into_iter()
        .map(responses::strip_response_item_ids_from_json)
        .collect::<Vec<_>>();
    assert_eq!(
        compact_input.last(),
        Some(&json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": COMPACT_PROMPT}]
        }))
    );
    let expected_summary = json!({
        "type": "message",
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": format!("{SUMMARY_PREFIX}\n{SUMMARY}")
        }]
    });
    assert!(
        requests[2]
            .input()
            .into_iter()
            .map(responses::strip_response_item_ids_from_json)
            .any(|item| item == expected_summary)
    );
    assert!(requests[2].body_contains_text("Remember the first user message."));
    assert!(requests[2].body_contains_text("Continue after compaction."));
    assert!(!requests[2].body_contains_text("The first answer."));
    Ok(())
}
