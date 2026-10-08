//! Native Local dispatch proofs: no network or remote skip gates.
use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::StructuredEditToolType;
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

const MODEL: &str = "grok-local-tools-fixture";
const CALL_ID: &str = "local-edit";
const PATCH: &str =
    "*** Begin Patch\n*** Update File: target.txt\n@@\n-before\n+after\n*** End Patch\n";

fn builder(home: Arc<TempDir>) -> TestCodexBuilder {
    test_codex()
        .with_home(home)
        .with_model(MODEL)
        .with_model_info_override(MODEL, |model| {
            model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
            model.structured_edit_tool_type = Some(StructuredEditToolType::ExactMatch);
        })
        .with_config(|config| {
            config.model_provider = config.model_providers["grok"].clone();
            config.model_provider.stream_max_retries = Some(0);
        })
}

fn home(server: &MockServer) -> Result<Arc<TempDir>> {
    let home = Arc::new(TempDir::new()?);
    fs::write(
        home.path().join("models.json"),
        serde_json::to_vec(&json!({"models": [{
            "slug": MODEL, "display_name": MODEL, "supported_reasoning_levels": [],
            "shell_type": "disabled", "visibility": "list", "supported_in_api": true,
            "priority": 0, "model_messages": {"instructions_template": "Use the requested local tool."},
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
web_search = "disabled"
[model_providers.grok]
name = "Grok"
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
    Ok(home)
}

fn tool_response(wire_name: &str, arguments: &str) -> String {
    let reasoning = json!({"type":"reasoning", "id":"rs_tools", "summary":[],
        "encrypted_content":"encrypted-replay", "content":[{"type":"reasoning_text","text":"private reasoning"}]});
    let call = json!({"type":"function_call", "id":"fc_tools", "call_id":CALL_ID,
        "name":wire_name, "arguments":arguments});
    let mut pending = call.clone();
    pending["arguments"] = json!("");
    responses::sse(vec![
        responses::ev_response_created("tool-response"),
        json!({"type":"response.output_item.added", "output_index":0, "item":reasoning}),
        json!({"type":"response.output_item.done", "output_index":0, "item":reasoning}),
        json!({"type":"response.output_item.added", "output_index":1, "item":pending}),
        json!({"type":"response.function_call_arguments.delta", "output_index":1, "delta":arguments}),
        json!({"type":"response.output_item.done", "output_index":1, "item":call}),
        responses::ev_completed("tool-response"),
    ])
}

fn answer(id: &str) -> String {
    let mut added = responses::ev_message_item_added(id, "");
    added["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message(id, "done");
    done["output_index"] = json!(0);
    responses::sse(vec![
        responses::ev_response_created(id),
        added,
        json!({"type":"response.output_text.delta", "output_index":0, "content_index":0, "delta":"done"}),
        done,
        responses::ev_completed(id),
    ])
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

#[test_case("function", "structured_edit"; "function")]
#[test_case("custom", "apply_patch"; "custom")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_local_edit_dispatch_and_cold_resume(kind: &str, tool: &str) -> Result<()> {
    let server = MockServer::start().await;
    let home = home(&server)?;
    let wire_name = flat_wire_name(kind, &ToolName::plain(tool));
    // Deliberately noncanonical JSON whitespace and key order must survive for functions.
    let arguments = if kind == "function" {
        "{ \"new_string\":\"after\", \"file_path\":\"target.txt\", \"old_string\":\"before\" }"
            .to_string()
    } else {
        json!({"patch":PATCH}).to_string()
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            tool_response(&wire_name, &arguments),
            answer("follow-up"),
            answer("cold-resume"),
        ],
    )
    .await;
    // Explicit Local executor: a process-wide remote setting must not alter this proof.
    let test = builder(Arc::clone(&home)).build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    test.submit_turn("Change target.txt from before to after.")
        .await?;
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    assert_eq!(
        mock.requests().len(),
        2,
        "the dispatched output must reach a follow-up inference"
    );

    let history = persisted(&test).await?;
    let pair = history
        .iter()
        .filter(|item| item["call_id"] == CALL_ID)
        .collect::<Vec<_>>();
    assert_eq!(pair.len(), 2);
    let call_type = if kind == "function" {
        "function_call"
    } else {
        "custom_tool_call"
    };
    assert_eq!(pair[0]["type"], call_type);
    assert_eq!(pair[0]["name"], tool);
    assert!(pair[0]["namespace"].is_null());
    assert_eq!(
        pair[0][if kind == "function" {
            "arguments"
        } else {
            "input"
        }],
        if kind == "function" {
            arguments.as_str()
        } else {
            PATCH
        }
    );
    assert_eq!(pair[1]["type"], format!("{call_type}_output"));

    let resumed = builder(home).restart(&server, &test).await?;
    resumed
        .submit_text_turn("Continue without editing again.")
        .await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        let body = request.body_json();
        assert_eq!(request.path(), "/v1/responses");
        let tools = body["tools"].as_array().expect("local declarations");
        assert!(
            tools
                .iter()
                .all(|tool| tool["type"] == "function" || tool["type"] == "x_search")
        );
        assert_eq!(
            tools
                .iter()
                .filter(|tool| tool["type"] == "x_search")
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!({"type":"x_search"})]
        );
        assert!(tools.iter().any(|tool| tool["name"] == wire_name));
    }
    for request in &requests[1..] {
        let input = request.input();
        let pair = input
            .iter()
            .filter(|item| item["call_id"] == CALL_ID)
            .collect::<Vec<_>>();
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0]["type"], "function_call");
        assert_eq!(pair[0]["name"], wire_name);
        assert_eq!(pair[0]["arguments"], arguments);
        assert_eq!(pair[1]["type"], "function_call_output");
        assert_eq!(
            pair[1]["output"],
            history
                .iter()
                .find(|item| item["type"] == format!("{call_type}_output"))
                .expect("persisted output")["output"]
        );
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == "reasoning")
                .cloned()
                .map(responses::strip_response_item_ids_from_json)
                .collect::<Vec<_>>(),
            vec![json!({"type":"reasoning", "summary":[], "encrypted_content":"encrypted-replay"})]
        );
        assert!(!request.body_contains_text("private reasoning"));
    }
    assert_eq!(
        persisted(&resumed)
            .await?
            .iter()
            .filter(|item| item["call_id"] == CALL_ID)
            .collect::<Vec<_>>(),
        pair,
        "request projection and cold replay must leave canonical durable history untouched"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "after\n"
    );
    Ok(())
}

#[test_case("{\"patch\":42}"; "wrong_wrapper_type")]
#[test_case("{\"patch\":\"*** Begin Patch\\n*** Update File: target.txt\\n@@\\n-before\\n+after\\n*** End Patch\\n\",\"extra\":true}"; "unknown_wrapper_key")]
#[test_case("{\"patch\":\"not a patch\"}"; "invalid_patch_grammar")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_malformed_custom_call_never_dispatches(arguments: &str) -> Result<()> {
    let server = MockServer::start().await;
    let wire_name = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    let mock = responses::mount_sse_once(&server, tool_response(&wire_name, arguments)).await;
    let test = builder(home(&server)?).build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Change target.txt.".into(),
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
    assert!(
        completed.error.is_some(),
        "malformed tool input must fail the turn"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::PatchApplyBegin(_))),
        "malformed calls must fail before patch dispatch"
    );
    assert_eq!(
        fs::read_to_string(test.workspace_path("target.txt"))?,
        "before\n"
    );
    assert_eq!(mock.requests().len(), 1);
    assert!(
        persisted(&test)
            .await?
            .iter()
            .all(|item| item["call_id"] != CALL_ID)
    );
    Ok(())
}
