//! Native Local hosted-search ingress, durable replay, and local dispatch proofs.
use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use codex_tools::ToolName;
use codex_tools::flat_wire_name;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use test_case::test_case;
use wiremock::MockServer;

const MODEL: &str = "grok-hosted-search-fixture";
const CALL_ID: &str = "local-edit";
const PATCH: &str =
    "*** Begin Patch\n*** Update File: target.txt\n@@\n-before\n+after\n*** End Patch\n";

fn builder(home: Arc<TempDir>) -> TestCodexBuilder {
    test_codex()
        .with_home(home)
        .with_model(MODEL)
        .with_model_info_override(MODEL, |model| {
            model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
            model.structured_edit_tool_type = None;
            model.web_search_tool_type = WebSearchToolType::Text;
        })
        .with_config(|config| {
            config.model_provider = config.model_providers["grok"].clone();
            config.model_provider.stream_max_retries = Some(0);
        })
}

fn home(
    server: &MockServer,
    domain_filter: &str,
    web_search_mode: WebSearchMode,
) -> Result<Arc<TempDir>> {
    let home = Arc::new(TempDir::new()?);
    fs::write(
        home.path().join("models.json"),
        serde_json::to_vec(&json!({"models": [{
            "slug": MODEL, "display_name": MODEL, "supported_reasoning_levels": [],
            "shell_type": "disabled", "visibility": "list", "supported_in_api": true,
            "priority": 0, "model_messages": {"instructions_template": "Use the requested search or local tool."},
            "support_verbosity": false, "apply_patch_tool_type": null,
            "truncation_policy": {"mode": "bytes", "limit": 10000},
            "experimental_supported_tools": [], "context_window": 1000000,
            "supports_reasoning_summary_parameter": false, "include_apps_usage_instructions": false,
            "node_repl_disabled": true, "tool_mode": "direct"
        }]}))?,
    )?;
    let base_url = format!("{}/v1", server.uri());
    fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model = "{MODEL}"
model_provider = "grok"
model_catalog_json = "models.json"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "{web_search_mode}"
[model_providers.grok]
name = "Grok"
base_url = "{base_url}"
wire_api = "grok_responses"
requires_openai_auth = false
supports_websockets = false
[model_providers.grok.x_search]
from_date = "2026-10-01"
to_date = "2026-10-08"
[tools.web_search]
{domain_filter} = ["example.com"]
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
standalone_web_search = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
"#
        ),
    )?;
    Ok(home)
}

fn hosted_items() -> Vec<Value> {
    let mut items = vec![json!({
        "type":"web_search_call", "id":"web-hosted", "status":"completed",
        "action":{"type":"search", "query":"fixture release notes"}
    })];
    for (index, name) in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ]
    .into_iter()
    .enumerate()
    {
        items.push(json!({
            "type":"custom_tool_call", "id":format!("x-hosted-{index}"),
            "call_id":format!("hosted-x-{index}"), "status":"completed", "name":name,
            "input":format!("{{ \"query\": \"fixture {index}\" }}")
        }));
    }
    items
}

fn response(id: &str, items: &[Value]) -> String {
    let mut events = vec![responses::ev_response_created(id)];
    for (index, item) in items.iter().enumerate() {
        let mut pending = item.clone();
        if pending.get("status").is_some() {
            pending["status"] = json!("in_progress");
        }
        events.push(json!({
            "type":"response.output_item.added", "output_index":index, "item":pending
        }));
        events.push(json!({
            "type":"response.output_item.done", "output_index":index, "item":item
        }));
    }
    let mut added = responses::ev_message_item_added(id, "");
    added["output_index"] = json!(items.len());
    let mut done = responses::ev_assistant_message(id, "done");
    done["output_index"] = json!(items.len());
    events.extend([
        added,
        json!({"type":"response.output_text.delta", "output_index":items.len(),
            "content_index":0, "delta":"done"}),
        done,
        responses::ev_completed(id),
    ]);
    responses::sse(events)
}

async fn persisted(test: &TestCodex) -> Result<Vec<Value>> {
    test.codex.ensure_rollout_materialized().await;
    test.codex.flush_rollout().await?;
    Ok(
        fs::read_to_string(test.codex.rollout_path().expect("durable rollout"))?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<serde_json::Result<Vec<_>>>()?
            .into_iter()
            .filter(|line| line["type"] == "response_item")
            .map(|line| line["payload"].clone())
            .collect(),
    )
}

fn search_history(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter(|item| {
            item["type"] == "web_search_call"
                || item["call_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("hosted-x-"))
        })
        .cloned()
        .collect()
}

#[test_case("allowed_domains", WebSearchMode::Cached; "cached_allowed")]
#[test_case("excluded_domains", WebSearchMode::Cached; "cached_excluded")]
#[test_case("allowed_domains", WebSearchMode::Indexed; "indexed_allowed")]
#[test_case("excluded_domains", WebSearchMode::Indexed; "indexed_excluded")]
#[test_case("allowed_domains", WebSearchMode::Live; "live_allowed")]
#[test_case("excluded_domains", WebSearchMode::Live; "live_excluded")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_hosted_search_survives_follow_up_local_dispatch_and_cold_resume(
    domain_filter: &str,
    web_search_mode: WebSearchMode,
) -> Result<()> {
    let server = MockServer::start().await;
    let home = home(&server, domain_filter, web_search_mode)?;
    let hosted = hosted_items();
    let reasoning = json!({"type":"reasoning", "id":"rs_hosted", "summary":[],
        "encrypted_content":"opaque-hosted-replay",
        "content":[{"type":"reasoning_text", "text":"private search reasoning"}]});
    let mut initial = vec![reasoning];
    initial.extend(hosted.clone());
    let wire_name = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    let arguments = json!({"patch":PATCH}).to_string();
    let local = json!({"type":"function_call", "id":"fc_local", "call_id":CALL_ID,
        "name":wire_name, "arguments":arguments});
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            response("hosted-search", &initial),
            responses::sse(vec![
                responses::ev_response_created("local-edit"),
                json!({"type":"response.output_item.added", "output_index":0, "item":local}),
                json!({"type":"response.output_item.done", "output_index":0, "item":local}),
                responses::ev_completed("local-edit"),
            ]),
            response("local-output", &[]),
            response("cold-resume", &[]),
        ],
    )
    .await;
    // Explicit Local executor: no process-wide remote setting or skip gate changes this proof.
    let test = builder(Arc::clone(&home)).build(&server).await?;
    assert_eq!(test.config.web_search_mode.value(), web_search_mode);
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    test.submit_text_turn("Search Web and X for the fixture release.")
        .await?;
    assert_eq!(
        mock.requests().len(),
        1,
        "hosted calls need no local follow-up"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "before\n"
    );
    let initial_history = persisted(&test).await?;
    assert_eq!(search_history(&initial_history), hosted);
    assert!(
        initial_history.iter().all(|item| {
            item["type"] != "function_call_output" && item["type"] != "custom_tool_call_output"
        }),
        "hosted completions must not synthesize local outputs"
    );

    test.submit_text_turn("Now change target.txt from before to after.")
        .await?;
    assert_eq!(
        mock.requests().len(),
        3,
        "only the local call requires another inference"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    let before_resume = persisted(&test).await?;
    assert_eq!(search_history(&before_resume), hosted);
    let local_pair = before_resume
        .iter()
        .filter(|item| item["call_id"] == CALL_ID)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(local_pair.len(), 2);
    assert_eq!(
        local_pair[0],
        json!({"type":"custom_tool_call", "id":"fc_local",
        "call_id":CALL_ID, "name":"apply_patch", "input":PATCH})
    );
    assert_eq!(local_pair[1]["type"], "custom_tool_call_output");

    let resumed = builder(home).restart(&server, &test).await?;
    assert_eq!(resumed.config.web_search_mode.value(), web_search_mode);
    resumed
        .submit_text_turn("Summarize the saved search without editing again.")
        .await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        assert_eq!(request.path(), "/v1/responses");
        let body = request.body_json();
        let tools = body["tools"].as_array().expect("mixed tool declarations");
        assert_eq!(
            tools
                .iter()
                .filter(|tool| tool["type"] != "function")
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                json!({"type":"web_search", "filters":{(domain_filter):["example.com"]}}),
                json!({"type":"x_search", "from_date":"2026-10-01", "to_date":"2026-10-08"}),
            ]
        );
        assert!(
            tools
                .iter()
                .any(|tool| tool["type"] == "function" && tool["name"] == wire_name)
        );
    }
    // Canonical completion status is durable; the retained Grok replay whitelist
    // omits status on the outgoing copy without changing identity or payload.
    let wire_hosted = hosted
        .iter()
        .cloned()
        .map(|mut item| {
            item.as_object_mut().unwrap().remove("status");
            item
        })
        .collect::<Vec<_>>();
    for request in &requests[1..] {
        let input = request.input();
        assert_eq!(
            search_history(&input),
            wire_hosted,
            "no synthetic output, flattening, or dropped search item"
        );
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == "reasoning")
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!({"type":"reasoning", "id":"rs_hosted", "summary":[],
                "encrypted_content":"opaque-hosted-replay"})]
        );
        assert!(!request.body_contains_text("private search reasoning"));
    }
    for request in &requests[2..] {
        let input = request.input();
        let pair = input
            .iter()
            .filter(|item| item["call_id"] == CALL_ID)
            .collect::<Vec<_>>();
        assert_eq!(pair.len(), 2);
        assert_eq!(
            pair[0],
            &json!({"type":"function_call", "id":"fc_local",
            "call_id":CALL_ID, "name":wire_name, "arguments":arguments})
        );
        assert_eq!(pair[1]["type"], "function_call_output");
        assert_eq!(pair[1]["output"], local_pair[1]["output"]);
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == "function_call_output"
                    || item["type"] == "custom_tool_call_output")
                .count(),
            1
        );
    }
    let after_resume = persisted(&resumed).await?;
    assert_eq!(search_history(&after_resume), hosted);
    assert_eq!(
        after_resume
            .iter()
            .filter(|item| item["call_id"] == CALL_ID)
            .cloned()
            .collect::<Vec<_>>(),
        local_pair
    );
    assert_eq!(
        after_resume
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .collect::<Vec<_>>(),
        initial_history
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .collect::<Vec<_>>(),
        "outbound opaque projection must not rewrite durable reasoning"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    Ok(())
}

#[test_case("oversized_x")]
#[test_case("unknown_x_name")]
#[test_case("missing_status")]
#[test_case("incomplete_status")]
#[test_case("missing_id")]
#[test_case("empty_id")]
#[test_case("empty_call_id")]
#[test_case("namespaced_x")]
#[test_case("raw_local_custom")]
#[test_case("web_missing_action")]
#[test_case("web_incomplete_status")]
#[test_case("web_missing_id")]
#[test_case("web_not_advertised")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_hosted_completion_fails_before_local_dispatch(case: &str) -> Result<()> {
    let server = MockServer::start().await;
    let mut item = if case.starts_with("web_") {
        hosted_items().remove(0)
    } else {
        hosted_items().remove(1)
    };
    match case {
        "oversized_x" => item["input"] = json!("q".repeat(40_000)),
        "unknown_x_name" => item["name"] = json!("x_search_unsupported"),
        "missing_status" => {
            item.as_object_mut().expect("item object").remove("status");
        }
        "incomplete_status" | "web_incomplete_status" => item["status"] = json!("in_progress"),
        "missing_id" | "web_missing_id" => {
            item.as_object_mut().expect("item object").remove("id");
        }
        "empty_id" => item["id"] = json!(""),
        "empty_call_id" => item["call_id"] = json!(""),
        "namespaced_x" => item["namespace"] = json!("functions"),
        "raw_local_custom" => {
            item["name"] = json!("apply_patch");
            item["input"] = json!(PATCH);
        }
        "web_missing_action" => {
            item.as_object_mut().expect("item object").remove("action");
        }
        "web_not_advertised" => {}
        _ => unreachable!("test cases are exhaustive"),
    }
    let mock = responses::mount_sse_once(&server, response("rejected-search", &[item])).await;
    let mut builder = builder(home(&server, "allowed_domains", WebSearchMode::Live)?);
    if case == "web_not_advertised" {
        builder = builder.with_config(|config| {
            config
                .web_search_mode
                .set(WebSearchMode::Disabled)
                .expect("disable search");
        });
    }
    let test = builder.build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Use the available tools.".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut events = Vec::new();
    let event = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let EventMsg::TurnComplete(completed) = event else {
        unreachable!()
    };
    assert!(completed.error.is_some(), "{case} must fail closed");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::PatchApplyBegin(_))),
        "raw custom calls must not reach a local executor"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "before\n"
    );
    assert_eq!(mock.requests().len(), 1);
    assert!(
        persisted(&test).await?.iter().all(|item| !matches!(
            item["type"].as_str(),
            Some(
                "custom_tool_call"
                    | "web_search_call"
                    | "function_call_output"
                    | "custom_tool_call_output"
            )
        )),
        "rejected provider calls and synthesized outputs must not enter canonical history"
    );
    Ok(())
}
