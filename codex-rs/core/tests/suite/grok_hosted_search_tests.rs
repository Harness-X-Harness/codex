//! Native Local hosted-search ingress, durable replay, and local dispatch proofs.
use anyhow::Context;
use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::turn_input::TurnInputSubmission;
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
        if item["type"] == "custom_tool_call" {
            events.push(
                json!({"type":"response.custom_tool_call_input.done","output_index":index,
                "item_id":item["id"],"input":item["input"]}),
            );
        }
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

fn with_turn_metadata(mut item: Value, turn_id: &str) -> Value {
    item["internal_chat_message_metadata_passthrough"] = json!({"turn_id":turn_id});
    item
}

async fn successful_turn_events(test: &TestCodex, prompt: &str) -> Result<(String, Vec<EventMsg>)> {
    let submission = test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let TurnInputSubmission::Started { turn_id } = submission else {
        panic!("expected a new turn, got {submission:?}");
    };
    let mut events = Vec::new();
    let event = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(completed) if completed.turn_id == turn_id)
    })
    .await;
    let EventMsg::TurnComplete(completed) = event else {
        unreachable!()
    };
    assert!(completed.error.is_none(), "turn failed: {completed:?}");
    Ok((turn_id, events))
}

#[derive(Clone, Copy)]
enum HostedCallTiming {
    LaterTurn,
    BeforeLocalOutput,
}

#[test_case(HostedCallTiming::LaterTurn; "later_turn")]
#[test_case(HostedCallTiming::BeforeLocalOutput; "before_local_output")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_search_reusing_local_call_id_survives_follow_up_and_cold_resume(
    timing: HostedCallTiming,
) -> Result<()> {
    let server = MockServer::start().await;
    let home = home(&server, "allowed_domains", WebSearchMode::Live)?;
    let wire_name = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    let arguments = json!({"patch":PATCH}).to_string();
    let local = json!({"type":"function_call", "id":"fc_local", "call_id":CALL_ID,
        "name":wire_name, "arguments":arguments});
    // The hosted completion deliberately reuses the earlier local call ID, not its item ID.
    let hosted = json!({"type":"custom_tool_call", "id":"x_reused_local_call",
        "call_id":CALL_ID, "status":"completed", "name":"x_keyword_search",
        "input":"{ \"query\" : \"fixture release\", \"count\" : 3 }"});
    let mut local_events = vec![
        responses::ev_response_created("local-edit"),
        json!({"type":"response.output_item.added", "output_index":0, "item":local}),
        json!({"type":"response.output_item.done", "output_index":0, "item":local}),
    ];
    if let HostedCallTiming::BeforeLocalOutput = timing {
        let mut pending = hosted.clone();
        pending["status"] = json!("in_progress");
        local_events.extend([
            json!({"type":"response.output_item.added", "output_index":1, "item":pending}),
            json!({"type":"response.custom_tool_call_input.done","output_index":1,"item_id":hosted["id"],"input":hosted["input"]}),
            json!({"type":"response.output_item.done", "output_index":1, "item":hosted}),
        ]);
    }
    local_events.push(responses::ev_completed("local-edit"));
    let mut mocked_responses = vec![responses::sse(local_events), response("local-output", &[])];
    if let HostedCallTiming::LaterTurn = timing {
        mocked_responses.push(response("hosted-search", std::slice::from_ref(&hosted)));
    }
    mocked_responses.extend([
        response("immediate-follow-up", &[]),
        response("cold-resume", &[]),
    ]);
    let expected_requests = mocked_responses.len();
    let mock = responses::mount_grok_sse_sequence(
        &server,
        mocked_responses,
        std::slice::from_ref(&hosted),
    )
    .await;
    // Explicit Local executor, as in the mixed hosted/local dispatch proof below.
    let test = builder(Arc::clone(&home)).build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    let (local_turn_id, mut events) =
        successful_turn_events(&test, "Change target.txt from before to after.").await?;
    assert_eq!(mock.requests().len(), 2);
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    let initial_history = persisted(&test)
        .await?
        .into_iter()
        .filter(|item| item["call_id"] == CALL_ID)
        .collect::<Vec<_>>();
    let local_output = initial_history
        .iter()
        .find(|item| item["type"] == "custom_tool_call_output")
        .expect("the real local edit must produce an output")
        .clone();
    assert_eq!(
        local_output["internal_chat_message_metadata_passthrough"]["turn_id"],
        local_turn_id
    );
    let canonical_local = with_turn_metadata(
        json!({"type":"custom_tool_call", "id":"fc_local",
        "call_id":CALL_ID, "name":"apply_patch", "input":PATCH}),
        &local_turn_id,
    );
    let mut canonical_hosted = with_turn_metadata(hosted.clone(), &local_turn_id);
    let initial_canonical = match timing {
        HostedCallTiming::LaterTurn => vec![canonical_local.clone(), local_output.clone()],
        HostedCallTiming::BeforeLocalOutput => vec![
            canonical_local.clone(),
            canonical_hosted.clone(),
            local_output.clone(),
        ],
    };
    assert_eq!(initial_history, initial_canonical);

    if let HostedCallTiming::LaterTurn = timing {
        let (hosted_turn_id, hosted_events) =
            successful_turn_events(&test, "Search X for the fixture release.").await?;
        events.extend(hosted_events);
        canonical_hosted = with_turn_metadata(hosted.clone(), &hosted_turn_id);
        assert_eq!(
            mock.requests().len(),
            3,
            "hosted calls need no local follow-up"
        );
    }
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                EventMsg::SearchActivity(activity)
                    if activity.kind == codex_protocol::SearchActivityKind::X =>
                    Some(activity.state),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![
            codex_protocol::SearchActivityState::Running,
            codex_protocol::SearchActivityState::Completed
        ]
    );
    assert_eq!(events.iter().filter(|event| matches!(event, EventMsg::ItemCompleted(event) if matches!(&event.item, codex_protocol::items::TurnItem::XSearch(item) if item.call_id == CALL_ID && item.name == "x_keyword_search"))).count(),1);
    let canonical = match timing {
        HostedCallTiming::LaterTurn => {
            vec![canonical_local, local_output.clone(), canonical_hosted]
        }
        HostedCallTiming::BeforeLocalOutput => {
            vec![canonical_local, canonical_hosted, local_output.clone()]
        }
    };
    events.extend(
        successful_turn_events(&test, "Summarize the saved search.")
            .await?
            .1,
    );
    assert_eq!(
        mock.requests().len(),
        expected_requests - 1,
        "the immediate replay must succeed"
    );
    let before_resume = persisted(&test).await?;

    let resumed = builder(home).restart(&server, &test).await?;
    events.extend(
        successful_turn_events(&resumed, "Continue without editing again.")
            .await?
            .1,
    );
    let after_resume = persisted(&resumed).await?;
    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        expected_requests,
        "the durable replay must succeed"
    );

    for history in [&before_resume, &after_resume] {
        assert_eq!(
            history
                .iter()
                .filter(|item| item["call_id"] == CALL_ID)
                .cloned()
                .collect::<Vec<_>>(),
            canonical,
            "the local pair and hosted identity, completion status, and input stay canonical"
        );
        assert_eq!(
            history
                .iter()
                .filter(|item| item["type"] == "function_call_output"
                    || item["type"] == "custom_tool_call_output")
                .count(),
            1,
            "hosted completions must not synthesize outputs"
        );
    }

    let wire_output = requests[1]
        .input()
        .into_iter()
        .find(|item| item["type"] == "function_call_output" && item["call_id"] == CALL_ID)
        .expect("the automatic follow-up must carry the local output");
    assert_eq!(wire_output["output"], local_output["output"]);
    let wire_local_pair = vec![local.clone(), wire_output.clone()];
    let mut wire_hosted = hosted;
    wire_hosted
        .as_object_mut()
        .expect("hosted item")
        .remove("status");
    let wire_history = match timing {
        HostedCallTiming::LaterTurn => vec![local, wire_output, wire_hosted],
        HostedCallTiming::BeforeLocalOutput => vec![local, wire_hosted, wire_output],
    };
    // Include the automatic local-output request: in the interleaved case it
    // already needs to replay both calls before any further user turn.
    for (index, request) in requests.iter().enumerate().skip(1) {
        let input = request.input();
        assert!(input.iter().all(|item| {
            item.get("internal_chat_message_metadata_passthrough")
                .is_none()
        }));
        let expected = if matches!(timing, HostedCallTiming::LaterTurn) && index < 3 {
            &wire_local_pair
        } else {
            &wire_history
        };
        assert_eq!(request.path(), "/v1/responses");
        assert_eq!(
            input
                .iter()
                .filter(|item| item["call_id"] == CALL_ID)
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            expected.as_slice(),
            "warm and cold replay must preserve the hosted item without flattening it"
        );
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == "function_call_output"
                    || item["type"] == "custom_tool_call_output")
                .count(),
            1
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, EventMsg::PatchApplyBegin(_)))
            .count(),
        1,
        "the real local edit must execute exactly once across hosted search and replay"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    Ok(())
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
    let completed_x_calls = hosted
        .iter()
        .filter(|item| item["type"] == "custom_tool_call")
        .cloned()
        .collect::<Vec<_>>();
    let mock = responses::mount_grok_sse_sequence(
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
        &completed_x_calls,
    )
    .await;
    // Explicit Local executor: no process-wide remote setting or skip gate changes this proof.
    let test = builder(Arc::clone(&home)).build(&server).await?;
    assert_eq!(test.config.web_search_mode.value(), web_search_mode);
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    let (hosted_turn_id, hosted_events) =
        successful_turn_events(&test, "Search Web and X for the fixture release.").await?;
    assert_eq!(hosted_events.iter().filter(|event| matches!(event, EventMsg::SearchActivity(activity) if activity.kind == codex_protocol::SearchActivityKind::X && activity.state == codex_protocol::SearchActivityState::Running)).count(),4);
    assert_eq!(hosted_events.iter().filter(|event| matches!(event, EventMsg::ItemCompleted(event) if matches!(event.item,codex_protocol::items::TurnItem::XSearch(_)))).count(),4);
    let canonical_hosted = hosted
        .iter()
        .cloned()
        .map(|item| with_turn_metadata(item, &hosted_turn_id))
        .collect::<Vec<_>>();
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
    assert_eq!(search_history(&initial_history), canonical_hosted);
    assert!(
        initial_history.iter().all(|item| {
            item["type"] != "function_call_output" && item["type"] != "custom_tool_call_output"
        }),
        "hosted completions must not synthesize local outputs"
    );

    let (local_turn_id, _) =
        successful_turn_events(&test, "Now change target.txt from before to after.").await?;
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
    assert_eq!(search_history(&before_resume), canonical_hosted);
    let local_pair = before_resume
        .iter()
        .filter(|item| item["call_id"] == CALL_ID)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(local_pair.len(), 2);
    assert_eq!(
        local_pair[0],
        with_turn_metadata(
            json!({"type":"custom_tool_call", "id":"fc_local",
        "call_id":CALL_ID, "name":"apply_patch", "input":PATCH}),
            &local_turn_id
        )
    );
    assert_eq!(local_pair[1]["type"], "custom_tool_call_output");
    assert_eq!(
        local_pair[1]["internal_chat_message_metadata_passthrough"]["turn_id"],
        local_turn_id
    );

    let resumed = builder(home).restart(&server, &test).await?;
    assert_eq!(resumed.config.web_search_mode.value(), web_search_mode);
    successful_turn_events(
        &resumed,
        "Summarize the saved search without editing again.",
    )
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
            item.as_object_mut()
                .expect("hosted item object")
                .remove("status");
            item
        })
        .collect::<Vec<_>>();
    for request in &requests[1..] {
        let input = request.input();
        assert!(input.iter().all(|item| {
            item.get("internal_chat_message_metadata_passthrough")
                .is_none()
        }));
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
    assert_eq!(search_history(&after_resume), canonical_hosted);
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

#[derive(Clone, Copy)]
enum SearchIdentityRewrite {
    Preserve,
    WebId,
    WebType,
    AssistantToPendingWeb,
}

impl codex_extension_api::TurnItemContributor for SearchIdentityRewrite {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a codex_extension_api::ExtensionData,
        _turn_store: &'a codex_extension_api::ExtensionData,
        item: &'a mut codex_protocol::items::TurnItem,
    ) -> codex_extension_api::ExtensionFuture<'a, Result<(), String>> {
        use codex_protocol::items::TurnItem;
        Box::pin(async move {
            match (self, &mut *item) {
                (Self::WebId, TurnItem::WebSearch(search)) => search.id = "rewritten-web".into(),
                (Self::WebType, TurnItem::WebSearch(_)) => {
                    *item = TurnItem::UserMessage(codex_protocol::items::UserMessageItem::new(&[]));
                }
                (Self::AssistantToPendingWeb, TurnItem::AgentMessage(_)) => {
                    *item = TurnItem::WebSearch(codex_protocol::items::WebSearchItem {
                        id: "web-hosted".into(),
                        query: "forged correlation".into(),
                        action: codex_protocol::models::WebSearchAction::Search {
                            query: Some("forged correlation".into()),
                            queries: None,
                        },
                        results: None,
                    });
                }
                _ => {}
            }
            Ok(())
        })
    }
}

#[test_case(SearchIdentityRewrite::WebId, ModeKind::Default; "web_id")]
#[test_case(SearchIdentityRewrite::WebType, ModeKind::Default; "web_type")]
#[test_case(SearchIdentityRewrite::AssistantToPendingWeb, ModeKind::Default; "earlier_message_steals_web_identity")]
#[test_case(SearchIdentityRewrite::AssistantToPendingWeb, ModeKind::Plan; "plan_mode_message_steals_web_identity")]
#[tokio::test]
async fn hosted_search_activity_rejects_contributor_rebinding_and_clears_preview(
    rewrite: SearchIdentityRewrite,
    mode: ModeKind,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let home = home(&server, "allowed_domains", WebSearchMode::Live)?;
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.turn_item_contributor(Arc::new(rewrite));
    let test = builder(home)
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let item = hosted_items()[0].clone();
    let mut pending = item.clone();
    pending["status"] = json!("in_progress");
    let mut message_added = responses::ev_message_item_added("message-before-search", "");
    message_added["output_index"] = json!(0);
    let mut message_done = responses::ev_assistant_message("message-before-search", "AB");
    message_done["output_index"] = json!(0);
    let mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("contributor-rebinding"),
            message_added,
            json!({"type":"response.output_text.delta","output_index":0,"delta":"A"}),
            json!({"type":"response.output_item.added","output_index":1,"item":pending}),
            json!({"type":"response.output_item.done","output_index":1,"item":item}),
            json!({"type":"response.output_text.delta","output_index":0,"delta":"B"}),
            message_done,
            responses::ev_completed("contributor-rebinding"),
        ]),
    )
    .await;
    let submitted = test
        .codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Search the fixture".into(),
                text_elements: vec![],
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                collaboration_mode: Some(CollaborationMode {
                    mode,
                    settings: Settings {
                        model: MODEL.into(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;
    let TurnInputSubmission::Started { turn_id } = submitted else {
        panic!("new turn expected")
    };
    let mut events = Vec::new();
    let terminal = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(done) if done.turn_id == turn_id)
    })
    .await;
    let EventMsg::TurnComplete(done) = terminal else {
        unreachable!()
    };
    assert!(done.error.is_some());
    let activity = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::SearchActivity(event) => Some(event.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        activity.iter().map(|event| event.state).collect::<Vec<_>>(),
        vec![
            codex_protocol::SearchActivityState::Running,
            codex_protocol::SearchActivityState::Completed,
            codex_protocol::SearchActivityState::Cleared,
        ]
    );
    assert!(
        activity
            .iter()
            .all(|event| event.attempt_id == activity[0].attempt_id
                && event.output_index == 1
                && event.item_id == "web-hosted")
    );
    assert!(!events.iter().any(|event| matches!(event,
        EventMsg::ItemCompleted(event) if matches!(event.item, codex_protocol::items::TurnItem::WebSearch(_)))));
    assert!(search_history(&persisted(&test).await?).is_empty());
    let request = mock.single_request().body_json();
    assert!(
        request["tools"]
            .as_array()
            .context("request tool declarations")?
            .iter()
            .any(|tool| tool["type"] == "web_search")
    );
    Ok(())
}

#[path = "grok_x_search_activity_tests.rs"]
mod x_activity;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_search_activity_cannot_replace_an_earlier_local_call_identity() -> Result<()> {
    let server = MockServer::start().await;
    let home = home(&server, "allowed_domains", WebSearchMode::Live)?;
    let local = json!({"type":"function_call", "id":"local-provider-id", "call_id":CALL_ID,
        "name":flat_wire_name("custom", &ToolName::plain("apply_patch")),
        "arguments":json!({"patch":PATCH}).to_string()});
    let local_response = responses::sse(vec![
        responses::ev_response_created("local-first"),
        json!({"type":"response.output_item.added","output_index":0,"item":local}),
        json!({"type":"response.output_item.done","output_index":0,"item":local}),
        responses::ev_completed("local-first"),
    ]);
    let mut colliding_web = hosted_items()[0].clone();
    colliding_web["id"] = json!(CALL_ID);
    colliding_web["action"]["query"] = json!("must not replace local edit");
    let mock = responses::mount_grok_sse_sequence(
        &server,
        vec![local_response, response("colliding-web", &[colliding_web])],
        &[],
    )
    .await;
    // Exercise the real local edit, as in the retained mixed/local C7 witness.
    let test = builder(home).build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Perform the fixture edit, then search.".into(),
            text_elements: vec![],
        }]))
        .await?;
    let mut events = Vec::new();
    let terminal = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let EventMsg::TurnComplete(done) = terminal else {
        unreachable!()
    };
    assert!(done.error.is_some());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::SearchActivity(_)))
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .input()
            .iter()
            .any(|item| item["type"] == "function_call_output" && item["call_id"] == CALL_ID)
    );
    let durable = persisted(&test).await?;
    assert!(search_history(&durable).is_empty());
    assert_eq!(
        durable
            .iter()
            .filter(|item| item["type"] == "custom_tool_call_output" && item["call_id"] == CALL_ID)
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn hosted_search_activity_guard_preserves_stock_plan_without_a_preview() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.turn_item_contributor(Arc::new(SearchIdentityRewrite::Preserve));
    let test = test_codex()
        .with_model("gpt-5.4")
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("stock-plan"),
            responses::ev_message_item_added("stock-plan-message", ""),
            responses::ev_assistant_message("stock-plan-message", "unchanged plan answer"),
            responses::ev_completed("stock-plan"),
        ]),
    )
    .await;
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Prepare the plan".into(),
                text_elements: vec![],
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Plan,
                    settings: Settings {
                        model: "gpt-5.4".into(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;
    let mut events = Vec::new();
    let terminal = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let EventMsg::TurnComplete(done) = terminal else {
        unreachable!()
    };
    assert!(done.error.is_none());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::SearchActivity(_)))
    );
    let messages = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemCompleted(event) => match &event.item {
                codex_protocol::items::TurnItem::AgentMessage(message) => Some(message),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "stock-plan-message");
    assert_eq!(
        serde_json::to_value(&messages[0].content)?,
        serde_json::to_value(vec![codex_protocol::items::AgentMessageContent::Text {
            text: "unchanged plan answer".into(),
        }])?
    );
    assert_eq!(mock.requests().len(), 1);
    Ok(())
}
