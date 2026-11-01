#![cfg(not(target_os = "windows"))]
#![allow(clippy::unwrap_used)]

use codex_core::TurnInputRequest;
use codex_core::config::Config;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall as ExtensionToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_features::Feature;
use core_test_support::test_codex::local_requests;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_exec_command_call_with_args;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::oneshot;

async fn run_turn(test: &TestCodex, prompt: &str) -> anyhow::Result<()> {
    let session_model = test.session_configured.model.clone();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.cwd.path());

    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: prompt.into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                environments: Some(local_requests(test.config.cwd.clone())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;

    wait_for_event(&test.codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    Ok(())
}

async fn build_codex_with_test_tool(server: &wiremock::MockServer) -> anyhow::Result<TestCodex> {
    let mut builder = test_codex().with_model("test-gpt-5.1-codex");
    builder.build(server).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_v2_tool_calls_with_the_same_id_run_in_parallel() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(|config| {
            config.features.enable(Feature::MultiAgentV2).unwrap();
        })
        .build(&server)
        .await?;
    let args = json!({
        "barrier": { "id": "parallel-test-sync", "participants": 2, "timeout_ms": 5_000 }
    })
    .to_string();
    let calls = sse(vec![
        ev_response_created("resp-1"),
        ev_function_call("shared", "test_sync_tool", &args),
        ev_function_call("shared", "test_sync_tool", &args),
        ev_completed("resp-1"),
    ]);
    let done = sse(vec![
        ev_assistant_message("msg-1", "done"),
        ev_completed("resp-2"),
    ]);
    let responses = mount_sse_sequence(&server, vec![calls, done]).await;

    run_turn(&test, "exercise sync tool").await?;
    // Either call can return "ok" only after both handlers reach the barrier.
    assert_eq!(
        responses.function_call_output_text("shared").as_deref(),
        Some("ok")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_tools_run_in_parallel() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.4");
    let test = builder.build(&server).await?;

    let shell_args = |call_id: &str| {
        json!({
            "cmd": format!(": > {call_id}.started; while [ ! -f release ]; do sleep 0.01; done; echo {call_id}"),
            "login": false,
            "yield_time_ms": 30_000,
        })
        .to_string()
    };
    let args_one = shell_args("call-1");
    let args_two = shell_args("call-2");

    let first_response = sse(vec![
        json!({"type": "response.created", "response": {"id": "resp-1"}}),
        ev_function_call("call-1", "exec_command", &args_one),
        ev_function_call("call-2", "exec_command", &args_two),
        ev_completed("resp-1"),
    ]);
    let second_response = sse(vec![
        ev_assistant_message("msg-1", "done"),
        ev_completed("resp-2"),
    ]);
    let responses = mount_sse_sequence(&server, vec![first_response, second_response]).await;

    let release_commands = async {
        let overlap = tokio::time::timeout(Duration::from_secs(5), async {
            while !test.cwd.path().join("call-1.started").exists()
                || !test.cwd.path().join("call-2.started").exists()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        fs::write(test.cwd.path().join("release"), "")?;
        overlap.map_err(|_| anyhow::anyhow!("shell commands did not overlap"))
    };
    let (turn_result, overlap_result) =
        tokio::join!(run_turn(&test, "run exec_command twice"), release_commands);
    overlap_result?;
    turn_result?;

    for call_id in ["call-1", "call-2"] {
        let output = responses
            .function_call_output_text(call_id)
            .expect("missing exec output");
        assert!(output.contains("Process exited with code 0"), "{output}");
        assert!(output.contains(&format!("Output:\n{call_id}")), "{output}");
    }

    Ok(())
}

// The marker paths are owned by this fixture, never supplied by a tool call.
#[derive(Clone)]
struct MixedToolGate {
    directory: PathBuf,
}

impl ToolContributor for MixedToolGate {
    fn tools(
        &self,
        _: &ExtensionData,
        _: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ExtensionToolCall<'call>>>> {
        vec![Arc::new(self.clone())]
    }
}

impl<'call> ToolExecutor<ExtensionToolCall<'call>> for MixedToolGate {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("mixed_overlap_gate")
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: "mixed_overlap_gate".to_owned(),
            description: "Wait for this test's shell to enter its handler.".to_owned(),
            strict: true,
            parameters: parse_tool_input_schema(&json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false})).expect("fixture schema"),
            output_schema: None,
            defer_loading: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ExtensionToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(async move {
            wait_for_mixed_shell(&self.directory, Duration::from_secs(5))
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            Ok(Box::new(JsonToolOutput::new(
                json!({"call_id": call.call_id, "overlap_confirmed": true}),
            )) as Box<dyn ToolOutput>)
        })
    }
}

struct ReleaseMixedShell(PathBuf);

impl Drop for ReleaseMixedShell {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "");
    }
}

async fn wait_for_mixed_shell(directory: &Path, limit: Duration) -> anyhow::Result<()> {
    let _release_on_cancel = ReleaseMixedShell(directory.join("release"));
    let overlap = tokio::time::timeout(limit, async {
        while !directory.join("shell.started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    // Release the real shell on both success and failure, without turning a
    // missing overlap into a successful result. Drop also covers cancellation.
    fs::write(directory.join("release"), "")?;
    overlap.map_err(|_| anyhow::anyhow!("mixed tool handlers did not overlap"))
}

#[tokio::test]
async fn mixed_tool_gate_rejects_serial_execution_and_releases_shell() -> anyhow::Result<()> {
    let directory = tempfile::TempDir::new()?;
    // In serialized execution the first handler must finish before the shell
    // can create its marker. That ordering must fail even if the shell starts later.
    let result = wait_for_mixed_shell(directory.path(), Duration::ZERO).await;
    assert!(result.is_err());
    assert!(directory.path().join("release").exists());
    fs::write(directory.path().join("shell.started"), "")?;
    assert!(
        result.is_err(),
        "late shell start must not retroactively prove overlap"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_parallel_tools_run_in_parallel() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let directory = tempfile::TempDir::new()?;
    let _release_on_failure = ReleaseMixedShell(directory.path().join("release"));
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    extensions.tool_contributor(Arc::new(MixedToolGate {
        directory: directory.path().to_owned(),
    }));
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_extensions(Arc::new(extensions.build()))
        .build(&server)
        .await?;
    let started = directory.path().join("shell.started");
    let release = directory.path().join("release");
    let started = shlex::try_join([started.to_str().expect("fixture path")])?;
    let release = shlex::try_join([release.to_str().expect("fixture path")])?;
    let shell_args = json!({
        "cmd": format!(": > {started}; while [ ! -f {release} ]; do sleep 0.01; done; echo mixed-overlap"),
        "login": false,
        "yield_time_ms": 30_000,
    }).to_string();
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call("call-1", "mixed_overlap_gate", "{}"),
                ev_function_call("call-2", "exec_command", &shell_args),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    run_turn(&test, "mix tools").await?;
    let gate_output = responses
        .function_call_output_text("call-1")
        .expect("bound gate result");
    let gate_output: Value = serde_json::from_str(&gate_output)?;
    assert_eq!(gate_output["call_id"], "call-1");
    assert_eq!(gate_output["overlap_confirmed"], true);
    let shell_output = responses
        .function_call_output_text("call-2")
        .expect("bound shell result");
    assert!(
        shell_output.contains("Process exited with code 0"),
        "{shell_output}"
    );
    assert!(shell_output.contains("mixed-overlap"), "{shell_output}");
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_results_grouped() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = build_codex_with_test_tool(&server).await?;

    let shell_args = serde_json::to_string(&json!({
        "cmd": "echo 'shell output'",
        "yield_time_ms": 1_000,
    }))?;

    mount_sse_once(
        &server,
        sse(vec![
            json!({"type": "response.created", "response": {"id": "resp-1"}}),
            ev_function_call("call-1", "exec_command", &shell_args),
            ev_function_call("call-2", "exec_command", &shell_args),
            ev_function_call("call-3", "exec_command", &shell_args),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let tool_output_request = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    )
    .await;

    run_turn(&test, "run shell three times").await?;

    let input = tool_output_request.single_request().input();

    // find all function_call inputs with indexes
    let function_calls = input
        .iter()
        .enumerate()
        .filter(|(_, item)| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .collect::<Vec<_>>();

    let function_call_outputs = input
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
        })
        .collect::<Vec<_>>();

    assert_eq!(function_calls.len(), 3);
    assert_eq!(function_call_outputs.len(), 3);

    for (index, _) in &function_calls {
        for (output_index, _) in &function_call_outputs {
            assert!(
                *index < *output_index,
                "all function calls must come before outputs"
            );
        }
    }

    // output should come in the order of the function calls
    let zipped = function_calls
        .iter()
        .zip(function_call_outputs.iter())
        .collect::<Vec<_>>();
    for (call, output) in zipped {
        assert_eq!(
            call.1.get("call_id").and_then(Value::as_str),
            output.1.get("call_id").and_then(Value::as_str)
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_tools_start_before_response_completed_when_stream_delayed() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let output_file = tempfile::NamedTempFile::new()?;
    let output_path = output_file.path();
    let first_response_id = "resp-1";
    let second_response_id = "resp-2";

    let command = format!(
        "perl -MTime::HiRes -e 'print int(Time::HiRes::time()*1000), \"\\n\"' >> \"{}\"",
        output_path.display()
    );
    // Use a non-login shell to avoid slow, user-specific shell init (e.g. zsh profiles)
    // from making this timing-based test flaky.
    let args = json!({
        "cmd": command,
        "login": false,
        "yield_time_ms": 5_000,
    });

    let first_chunk = sse(vec![
        ev_response_created(first_response_id),
        ev_exec_command_call_with_args("call-1", &args),
        ev_exec_command_call_with_args("call-2", &args),
        ev_exec_command_call_with_args("call-3", &args),
        ev_exec_command_call_with_args("call-4", &args),
    ]);
    let second_chunk = sse(vec![ev_completed(first_response_id)]);
    let follow_up = sse(vec![
        ev_assistant_message("msg-1", "done"),
        ev_completed(second_response_id),
    ]);

    let (first_gate_tx, first_gate_rx) = oneshot::channel();
    let (completion_gate_tx, completion_gate_rx) = oneshot::channel();
    let (follow_up_gate_tx, follow_up_gate_rx) = oneshot::channel();
    let (streaming_server, completion_receivers) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: Some(first_gate_rx),
                body: first_chunk,
            },
            StreamingSseChunk {
                gate: Some(completion_gate_rx),
                body: second_chunk,
            },
        ],
        vec![StreamingSseChunk {
            gate: Some(follow_up_gate_rx),
            body: follow_up,
        }],
    ])
    .await;

    let mut builder = test_codex().with_model("gpt-5.4");
    let test = builder
        .build_with_streaming_server(&streaming_server)
        .await?;

    let session_model = test.session_configured.model.clone();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.cwd.path());
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "stream delayed completion".into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                environments: Some(local_requests(test.config.cwd.clone())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;

    let _ = first_gate_tx.send(());
    let _ = follow_up_gate_tx.send(());

    let timestamps = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let contents = fs::read_to_string(output_path)?;
            let timestamps = contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    line.trim()
                        .parse::<i64>()
                        .map_err(|err| anyhow::anyhow!("invalid timestamp {line:?}: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if timestamps.len() == 4 {
                return Ok::<_, anyhow::Error>(timestamps);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;

    let _ = completion_gate_tx.send(());
    wait_for_event(&test.codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let mut completion_iter = completion_receivers.into_iter();
    let completed_at = completion_iter
        .next()
        .expect("completion receiver missing")
        .await
        .expect("completion timestamp missing");
    let count = i64::try_from(timestamps.len()).expect("timestamp count fits in i64");
    assert_eq!(count, 4);

    for timestamp in timestamps {
        assert!(
            timestamp <= completed_at,
            "timestamp {timestamp} should be before or equal to completed {completed_at}"
        );
    }

    streaming_server.shutdown().await;

    Ok(())
}
