use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::model::ExecutionStatus;
use crate::model::ExecutionWindow;
use crate::model::TerminalModelObservation;
use crate::model::TerminalObservationSource;
use crate::model::TerminalOperation;
use crate::model::TerminalOperationKind;
use crate::model::TerminalRequest;
use crate::model::TerminalResult;
use crate::model::TerminalSession;
use crate::model::ToolCallKind;
use crate::model::ToolCallSummary;
use crate::payload::RawPayloadKind;
use crate::payload::RawPayloadRef;
use crate::raw_event::RawTraceEventPayload;
use crate::reducer::test_support::create_started_writer;
use crate::reducer::test_support::generic_summary;
use crate::reducer::test_support::message;
use crate::reducer::test_support::start_turn;
use crate::reducer::test_support::trace_context;
use crate::replay_bundle;
use crate::writer::TraceWriter;

#[test]
fn exec_tool_reduces_to_terminal_operation_and_session() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = create_started_writer(&temp)?;
    start_turn(&writer, "turn-1")?;
    append_inference_with_tool_call(&writer)?;

    let invocation_payload = writer.write_json_payload(
        RawPayloadKind::ToolInvocation,
        &json!({
            "tool_name": "exec_command",
            "tool_namespace": null,
            "payload": {
                "type": "function",
                "arguments": "{\"cmd\":\"cargo test\"}"
            }
        }),
    )?;
    let invocation_payload_id = invocation_payload.raw_payload_id.clone();
    let _tool_start = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: "tool-1".to_string(),
            model_visible_call_id: Some("call-1".to_string()),
            code_mode_runtime_tool_id: None,
            requester: crate::raw_event::RawToolCallRequester::Model,
            kind: ToolCallKind::ExecCommand,
            summary: generic_summary("exec_command"),
            invocation_payload: Some(invocation_payload),
        },
    )?;

    let runtime_start_payload = writer.write_json_payload(
        RawPayloadKind::ToolRuntimeEvent,
        &json!({
            "call_id": "tool-1",
            "turn_id": "turn-1",
            "command": ["cargo", "test"],
            "cwd": "/repo"
        }),
    )?;
    let runtime_start_payload_id = runtime_start_payload.raw_payload_id.clone();
    let runtime_start = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallRuntimeStarted {
            tool_call_id: "tool-1".to_string(),
            runtime_payload: runtime_start_payload,
        },
    )?;

    let runtime_end_payload = writer.write_json_payload(
        RawPayloadKind::ToolRuntimeEvent,
        &json!({
            "call_id": "tool-1",
            "process_id": "pty-1",
            "turn_id": "turn-1",
            "command": ["cargo", "test"],
            "cwd": "/repo",
            "stdout": "ok\n",
            "stderr": "",
            "exit_code": 0,
            "formatted_output": "ok\n",
            "status": "completed"
        }),
    )?;
    let runtime_end_payload_id = runtime_end_payload.raw_payload_id.clone();
    let runtime_end = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallRuntimeEnded {
            tool_call_id: "tool-1".to_string(),
            status: ExecutionStatus::Completed,
            runtime_payload: runtime_end_payload,
        },
    )?;

    let result_payload = writer.write_json_payload(
        RawPayloadKind::ToolResult,
        &json!({
            "type": "direct_response",
            "response_item": {
                "type": "function_call_output",
                "call_id": "call-1",
                "output": "ok\n"
            }
        }),
    )?;
    let result_payload_id = result_payload.raw_payload_id.clone();
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallEnded {
            tool_call_id: "tool-1".to_string(),
            status: ExecutionStatus::Completed,
            result_payload: Some(result_payload),
        },
    )?;

    start_turn(&writer, "turn-2")?;
    append_followup_with_tool_output(&writer)?;

    let rollout = replay_bundle(temp.path())?;
    let operation_id = "terminal_operation:1".to_string();
    let output_item_id = rollout.inference_calls["inference-2"]
        .request_item_ids
        .last()
        .expect("tool output item")
        .clone();

    assert_eq!(
        rollout.tool_calls["tool-1"].terminal_operation_id,
        Some(operation_id.clone()),
    );
    assert_eq!(
        rollout.tool_calls["tool-1"].raw_invocation_payload_id,
        Some(invocation_payload_id),
    );
    assert_eq!(
        rollout.tool_calls["tool-1"].raw_result_payload_id,
        Some(result_payload_id),
    );
    assert_eq!(
        rollout.tool_calls["tool-1"].raw_runtime_payload_ids,
        vec![
            runtime_start_payload_id.clone(),
            runtime_end_payload_id.clone()
        ],
    );
    assert_eq!(
        rollout.tool_calls["tool-1"].summary,
        ToolCallSummary::Terminal {
            operation_id: operation_id.clone(),
        },
    );
    assert_eq!(
        rollout.terminal_operations[&operation_id],
        TerminalOperation {
            operation_id: operation_id.clone(),
            terminal_id: Some("pty-1".to_string()),
            tool_call_id: "tool-1".to_string(),
            kind: TerminalOperationKind::ExecCommand,
            execution: ExecutionWindow {
                started_at_unix_ms: runtime_start.wall_time_unix_ms,
                started_seq: runtime_start.seq,
                ended_at_unix_ms: Some(runtime_end.wall_time_unix_ms),
                ended_seq: Some(runtime_end.seq),
                status: ExecutionStatus::Completed,
            },
            request: TerminalRequest::ExecCommand {
                command: vec!["cargo".to_string(), "test".to_string()],
                display_command: "cargo test".to_string(),
                cwd: "/repo".to_string(),
                yield_time_ms: None,
                max_output_tokens: None,
            },
            result: Some(TerminalResult {
                exit_code: Some(0),
                stdout: "ok\n".to_string(),
                stderr: String::new(),
                formatted_output: Some("ok\n".to_string()),
                original_token_count: None,
                chunk_id: None,
            }),
            model_observations: vec![TerminalModelObservation {
                call_item_ids: rollout.inference_calls["inference-1"]
                    .response_item_ids
                    .clone(),
                output_item_ids: vec![output_item_id],
                source: TerminalObservationSource::DirectToolCall,
            }],
            raw_payload_ids: vec![runtime_start_payload_id, runtime_end_payload_id],
        },
    );
    assert_eq!(
        rollout.terminal_sessions["pty-1"],
        TerminalSession {
            terminal_id: "pty-1".to_string(),
            thread_id: "thread-root".to_string(),
            created_by_operation_id: operation_id.clone(),
            operation_ids: vec![operation_id],
            execution: ExecutionWindow {
                started_at_unix_ms: runtime_start.wall_time_unix_ms,
                started_seq: runtime_start.seq,
                ended_at_unix_ms: None,
                ended_seq: None,
                status: ExecutionStatus::Running,
            },
        },
    );

    Ok(())
}

#[test]
fn write_stdin_operation_reuses_existing_terminal_session() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = create_started_writer(&temp)?;
    start_turn(&writer, "turn-1")?;

    let startup_payload = writer.write_json_payload(
        RawPayloadKind::ToolRuntimeEvent,
        &json!({
            "call_id": "tool-start",
            "process_id": "pty-1",
            "turn_id": "turn-1",
            "command": ["bash"],
            "cwd": "/repo"
        }),
    )?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: "tool-start".to_string(),
            model_visible_call_id: None,
            code_mode_runtime_tool_id: None,
            requester: crate::raw_event::RawToolCallRequester::Model,
            kind: ToolCallKind::ExecCommand,
            summary: generic_summary("exec_command"),
            invocation_payload: None,
        },
    )?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallRuntimeStarted {
            tool_call_id: "tool-start".to_string(),
            runtime_payload: startup_payload,
        },
    )?;

    let stdin_payload = writer.write_json_payload(
        RawPayloadKind::ToolRuntimeEvent,
        &json!({
            "call_id": "tool-stdin",
            "process_id": "pty-1",
            "turn_id": "turn-1",
            "command": ["bash"],
            "cwd": "/repo",
            "interaction_input": "echo hi\n"
        }),
    )?;
    let _stdin_start = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: "tool-stdin".to_string(),
            model_visible_call_id: None,
            code_mode_runtime_tool_id: None,
            requester: crate::raw_event::RawToolCallRequester::Model,
            kind: ToolCallKind::WriteStdin,
            summary: generic_summary("write_stdin"),
            invocation_payload: None,
        },
    )?;
    let stdin_runtime_start = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallRuntimeStarted {
            tool_call_id: "tool-stdin".to_string(),
            runtime_payload: stdin_payload,
        },
    )?;

    let rollout = replay_bundle(temp.path())?;
    let startup_operation_id = "terminal_operation:1".to_string();
    let stdin_operation_id = "terminal_operation:2".to_string();

    assert_eq!(
        rollout.terminal_sessions["pty-1"].operation_ids,
        vec![startup_operation_id, stdin_operation_id.clone()],
    );
    assert_eq!(
        rollout.terminal_operations[&stdin_operation_id],
        TerminalOperation {
            operation_id: stdin_operation_id.clone(),
            terminal_id: Some("pty-1".to_string()),
            tool_call_id: "tool-stdin".to_string(),
            kind: TerminalOperationKind::WriteStdin,
            execution: ExecutionWindow {
                started_at_unix_ms: stdin_runtime_start.wall_time_unix_ms,
                started_seq: stdin_runtime_start.seq,
                ended_at_unix_ms: None,
                ended_seq: None,
                status: ExecutionStatus::Running,
            },
            request: TerminalRequest::WriteStdin {
                stdin: "echo hi\n".to_string(),
                yield_time_ms: None,
                max_output_tokens: None,
            },
            result: None,
            model_observations: Vec::new(),
            raw_payload_ids: vec!["raw_payload:2".to_string()],
        },
    );

    Ok(())
}

#[test]
fn dispatch_write_stdin_payload_reduces_to_terminal_operation() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = create_started_writer(&temp)?;
    start_turn(&writer, "turn-1")?;

    let request_payload = writer.write_json_payload(
        RawPayloadKind::ToolInvocation,
        &json!({
            "tool_name": "write_stdin",
            "tool_namespace": null,
            "payload": {
                "type": "function",
                "arguments": json!({
                    "session_id": 123,
                    "chars": "echo hi\n",
                    "yield_time_ms": 250,
                    "max_output_tokens": 2000
                }).to_string()
            }
        }),
    )?;
    let request_payload_id = request_payload.raw_payload_id.clone();
    let tool_start = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: "tool-stdin".to_string(),
            model_visible_call_id: Some("call-stdin".to_string()),
            code_mode_runtime_tool_id: None,
            requester: crate::raw_event::RawToolCallRequester::Model,
            kind: ToolCallKind::WriteStdin,
            summary: generic_summary("write_stdin"),
            invocation_payload: Some(request_payload),
        },
    )?;

    let response_payload = writer.write_json_payload(
        RawPayloadKind::ToolResult,
        &json!({
            "type": "direct_response",
            "response_item": {
                "type": "function_call_output",
                "call_id": "call-stdin",
                "output": "hi\n"
            }
        }),
    )?;
    let response_payload_id = response_payload.raw_payload_id.clone();
    let tool_end = writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallEnded {
            tool_call_id: "tool-stdin".to_string(),
            status: ExecutionStatus::Completed,
            result_payload: Some(response_payload),
        },
    )?;

    let rollout = replay_bundle(temp.path())?;
    let operation_id = "terminal_operation:1".to_string();

    assert_eq!(
        rollout.tool_calls["tool-stdin"].terminal_operation_id,
        Some(operation_id.clone()),
    );
    assert_eq!(
        rollout.tool_calls["tool-stdin"].summary,
        ToolCallSummary::Terminal {
            operation_id: operation_id.clone(),
        },
    );
    assert_eq!(
        rollout.terminal_operations[&operation_id],
        TerminalOperation {
            operation_id: operation_id.clone(),
            terminal_id: Some("123".to_string()),
            tool_call_id: "tool-stdin".to_string(),
            kind: TerminalOperationKind::WriteStdin,
            execution: ExecutionWindow {
                started_at_unix_ms: tool_start.wall_time_unix_ms,
                started_seq: tool_start.seq,
                ended_at_unix_ms: Some(tool_end.wall_time_unix_ms),
                ended_seq: Some(tool_end.seq),
                status: ExecutionStatus::Completed,
            },
            request: TerminalRequest::WriteStdin {
                stdin: "echo hi\n".to_string(),
                yield_time_ms: Some(250),
                max_output_tokens: Some(2000),
            },
            result: Some(TerminalResult {
                exit_code: None,
                stdout: "hi\n".to_string(),
                stderr: String::new(),
                formatted_output: Some("hi\n".to_string()),
                original_token_count: None,
                chunk_id: None,
            }),
            model_observations: Vec::new(),
            raw_payload_ids: vec![request_payload_id, response_payload_id],
        },
    );
    assert_eq!(
        rollout.terminal_sessions["123"],
        TerminalSession {
            terminal_id: "123".to_string(),
            thread_id: "thread-root".to_string(),
            created_by_operation_id: operation_id.clone(),
            operation_ids: vec![operation_id],
            execution: ExecutionWindow {
                started_at_unix_ms: tool_start.wall_time_unix_ms,
                started_seq: tool_start.seq,
                ended_at_unix_ms: None,
                ended_seq: None,
                status: ExecutionStatus::Running,
            },
        },
    );

    Ok(())
}

#[test]
fn code_mode_write_stdin_result_projects_structured_exec_fields() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = create_started_writer(&temp)?;
    start_turn(&writer, "turn-1")?;

    let request_payload = writer.write_json_payload(
        RawPayloadKind::ToolInvocation,
        &json!({
            "tool_name": "write_stdin",
            "tool_namespace": null,
            "payload": {
                "type": "function",
                "arguments": json!({
                    "session_id": 456,
                    "chars": "",
                    "yield_time_ms": 1000,
                    "max_output_tokens": 4000
                }).to_string()
            }
        }),
    )?;
    let response_payload = writer.write_json_payload(
        RawPayloadKind::ToolResult,
        &json!({
            "type": "code_mode_response",
            "value": {
                "chunk_id": "abc123",
                "wall_time_seconds": 1.25,
                "exit_code": 0,
                "original_token_count": 3,
                "output": "done\n"
            }
        }),
    )?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::CodeCellStarted {
            runtime_cell_id: "cell-1".to_string(),
            model_visible_call_id: "call-code".to_string(),
            source_js: "await tools.write_stdin({ chars: '' })".to_string(),
        },
    )?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: "tool-stdin".to_string(),
            model_visible_call_id: None,
            code_mode_runtime_tool_id: Some("runtime-tool-1".to_string()),
            requester: crate::raw_event::RawToolCallRequester::CodeCell {
                runtime_cell_id: "cell-1".to_string(),
            },
            kind: ToolCallKind::WriteStdin,
            summary: generic_summary("write_stdin"),
            invocation_payload: Some(request_payload),
        },
    )?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallEnded {
            tool_call_id: "tool-stdin".to_string(),
            status: ExecutionStatus::Completed,
            result_payload: Some(response_payload),
        },
    )?;

    let rollout = replay_bundle(temp.path())?;
    assert_eq!(
        rollout.terminal_operations["terminal_operation:1"].result,
        Some(TerminalResult {
            exit_code: Some(0),
            stdout: "done\n".to_string(),
            stderr: String::new(),
            formatted_output: Some("done\n".to_string()),
            original_token_count: Some(3),
            chunk_id: Some("abc123".to_string()),
        }),
    );

    Ok(())
}

#[test]
fn whole_number_stdin_replay_joins_runtime_sessions_and_preserves_limits() -> anyhow::Result<()> {
    for (runtime_id, aliases) in [
        ("123", ["123", "123.0", "1.23e2", "12300e-2"]),
        ("0", ["0", "-0.0", "0e999", "-0e-999"]),
        (
            "2147483647",
            [
                "2147483647",
                "2147483647.0",
                "2.147483647e9",
                "21474836470e-1",
            ],
        ),
        (
            "-2147483648",
            [
                "-2147483648",
                "-2147483648.0",
                "-2.147483648e9",
                "-21474836480e-1",
            ],
        ),
    ] {
        let temp = TempDir::new()?;
        let writer = create_started_writer(&temp)?;
        start_turn(&writer, "turn-1")?;
        writer.append_with_context(
            trace_context("turn-1"),
            RawTraceEventPayload::ToolCallStarted {
                tool_call_id: "tool-exec".to_string(),
                model_visible_call_id: None,
                code_mode_runtime_tool_id: None,
                requester: crate::raw_event::RawToolCallRequester::Model,
                kind: ToolCallKind::ExecCommand,
                summary: generic_summary("exec_command"),
                invocation_payload: None,
            },
        )?;
        let runtime_payload = writer.write_json_payload(
            RawPayloadKind::ToolRuntimeEvent,
            &json!({"process_id": runtime_id, "command": ["bash"], "cwd": "/repo"}),
        )?;
        writer.append_with_context(
            trace_context("turn-1"),
            RawTraceEventPayload::ToolCallRuntimeStarted {
                tool_call_id: "tool-exec".to_string(),
                runtime_payload,
            },
        )?;

        let mut recorded = Vec::new();
        for (index, session_id) in aliases.into_iter().enumerate() {
            let (limits, yield_time_ms, max_output_tokens) = match index {
                0 => (
                    r#", "yield_time_ms": 2.5e2, "max_output_tokens": 2e3"#.to_string(),
                    Some(250),
                    Some(2000),
                ),
                1 => (
                    format!(
                        r#", "yield_time_ms": {}.0, "max_output_tokens": {}.0"#,
                        u64::MAX,
                        usize::MAX,
                    ),
                    Some(u64::MAX),
                    Some(usize::MAX),
                ),
                2 => (String::new(), None, None),
                _ => (
                    r#", "yield_time_ms": null, "max_output_tokens": null"#.to_string(),
                    None,
                    None,
                ),
            };
            let tool_call_id = format!("stdin-{index}");
            let arguments =
                format!(r#"{{ "session_id": {session_id}, "chars": "echo hi\n"{limits} }}"#);
            let payloads = append_dispatch_stdin(
                &writer,
                &tool_call_id,
                &arguments,
                ExecutionStatus::Completed,
                json!({
                    "type": "direct_response",
                    "response_item": {
                        "type": "function_call_output",
                        "call_id": tool_call_id,
                        "output": "hi\n",
                    },
                }),
            )?;
            recorded.push((tool_call_id, yield_time_ms, max_output_tokens, payloads));
        }

        let rollout = replay_bundle(temp.path())?;
        assert_eq!(rollout.terminal_sessions.len(), 1);
        assert_eq!(rollout.terminal_operations.len(), 5);
        let mut operation_ids = vec!["terminal_operation:1".to_string()];
        for (tool_call_id, yield_time_ms, max_output_tokens, payloads) in recorded {
            let tool = &rollout.tool_calls[&tool_call_id];
            let operation_id = tool.terminal_operation_id.as_ref().expect("stdin operation");
            let operation = &rollout.terminal_operations[operation_id];
            operation_ids.push(operation_id.clone());
            assert_eq!(
                (
                    &operation.terminal_id,
                    &operation.request,
                    &operation.raw_payload_ids,
                ),
                (
                    &Some(runtime_id.to_string()),
                    &TerminalRequest::WriteStdin {
                        stdin: "echo hi\n".to_string(),
                        yield_time_ms,
                        max_output_tokens,
                    },
                    &payloads
                        .iter()
                        .map(|(reference, _)| reference.raw_payload_id.clone())
                        .collect::<Vec<_>>(),
                ),
            );
            assert_eq!(
                (&tool.raw_invocation_payload_id, &tool.raw_result_payload_id),
                (
                    &Some(payloads[0].0.raw_payload_id.clone()),
                    &Some(payloads[1].0.raw_payload_id.clone()),
                ),
            );
            for (reference, original) in payloads {
                assert_eq!(rollout.raw_payloads[&reference.raw_payload_id], reference);
                assert_eq!(
                    std::fs::read(temp.path().join(&reference.path))?,
                    serde_json::to_vec_pretty(&original)?,
                );
            }
        }
        assert_eq!(
            rollout.terminal_sessions[runtime_id].operation_ids,
            operation_ids,
        );
    }
    Ok(())
}

#[test]
fn whole_number_stdin_replay_preserves_historical_keys_and_raw_evidence() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = create_started_writer(&temp)?;
    start_turn(&writer, "turn-1")?;
    let mut recorded = Vec::new();
    for (session_id, expected_key) in [
        ("123.0", "123"),
        (r#""123.0""#, "123.0"),
        (r#""1.23e2""#, "1.23e2"),
        (r#""pty-1""#, "pty-1"),
        (r#"" 123 ""#, " 123 "),
        ("123.5", "123.5"),
        ("123.50", "123.50"),
        ("123.00000000000000000001", "123.00000000000000000001"),
        ("2147483648", "2147483648"),
        ("2147483648.0", "2147483648.0"),
        ("-2147483649", "-2147483649"),
        ("9007199254740993", "9007199254740993"),
        ("9007199254740993.0", "9007199254740993.0"),
        ("1E100", "1e+100"),
    ] {
        let tool_call_id = format!("stdin-{}", recorded.len());
        let arguments = format!(r#"{{"session_id":{session_id}}}"#);
        let payloads = append_dispatch_stdin(
            &writer,
            &tool_call_id,
            &arguments,
            ExecutionStatus::Failed,
            json!({"type": "error", "error": "historical stdin failure"}),
        )?;
        recorded.push((tool_call_id, expected_key, payloads));
    }

    let rollout = replay_bundle(temp.path())?;
    assert_eq!(rollout.terminal_sessions.len(), recorded.len());
    assert_eq!(rollout.terminal_operations.len(), recorded.len());
    for (tool_call_id, expected_key, payloads) in recorded {
        let tool = &rollout.tool_calls[&tool_call_id];
        let operation_id = tool
            .terminal_operation_id
            .as_ref()
            .expect("historical stdin operation");
        let operation = &rollout.terminal_operations[operation_id];
        assert_eq!(operation.terminal_id.as_deref(), Some(expected_key));
        assert_eq!(
            rollout.terminal_sessions[expected_key].operation_ids,
            vec![operation_id.clone()],
        );
        assert_eq!(operation.execution.status, ExecutionStatus::Failed);
        assert_eq!(
            operation.result,
            Some(TerminalResult {
                exit_code: None,
                stdout: String::new(),
                stderr: "historical stdin failure".to_string(),
                formatted_output: Some("historical stdin failure".to_string()),
                original_token_count: None,
                chunk_id: None,
            }),
        );
        assert_eq!(
            operation.raw_payload_ids,
            payloads
                .iter()
                .map(|(reference, _)| reference.raw_payload_id.clone())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            (&tool.raw_invocation_payload_id, &tool.raw_result_payload_id),
            (
                &Some(payloads[0].0.raw_payload_id.clone()),
                &Some(payloads[1].0.raw_payload_id.clone()),
            ),
        );
        for (reference, original) in payloads {
            assert_eq!(rollout.raw_payloads[&reference.raw_payload_id], reference);
            assert_eq!(
                std::fs::read(temp.path().join(&reference.path))?,
                serde_json::to_vec_pretty(&original)?,
            );
        }
    }
    Ok(())
}

#[test]
fn whole_number_stdin_replay_rejects_invalid_limits_and_missing_session_keys() -> anyhow::Result<()> {
    let mut arguments = Vec::new();
    for field in ["yield_time_ms", "max_output_tokens"] {
        for value in [
            "-1",
            "0.5",
            "1e-1",
            "1.00000000000000000001",
            "18446744073709551616",
            r#""250""#,
            "true",
            "[]",
            "{}",
        ] {
            arguments.push(format!(r#"{{"session_id":123,"{field}":{value}}}"#));
        }
    }
    arguments.extend(
        [
            "{}",
            r#"{"session_id":null}"#,
            r#"{"session_id":""}"#,
            r#"{"session_id":true}"#,
            r#"{"session_id":[]}"#,
            r#"{"session_id":{}}"#,
        ]
        .map(str::to_string),
    );
    for arguments in arguments {
        let temp = TempDir::new()?;
        let writer = create_started_writer(&temp)?;
        start_turn(&writer, "turn-1")?;
        append_dispatch_stdin(
            &writer,
            "stdin-invalid",
            &arguments,
            ExecutionStatus::Failed,
            json!({"type": "error", "error": "invalid stdin arguments"}),
        )?;
        let error = replay_bundle(temp.path()).expect_err(&arguments);
        assert!(
            format!("{error:#}").contains("parse terminal invocation payload"),
            "{arguments}: {error:#}",
        );
    }
    Ok(())
}

fn append_dispatch_stdin(
    writer: &TraceWriter,
    tool_call_id: &str,
    arguments: &str,
    status: ExecutionStatus,
    response: serde_json::Value,
) -> anyhow::Result<[(RawPayloadRef, serde_json::Value); 2]> {
    let invocation = json!({
        "tool_name": "write_stdin",
        "tool_namespace": null,
        "payload": {"type": "function", "arguments": arguments}
    });
    let invocation_payload = writer.write_json_payload(RawPayloadKind::ToolInvocation, &invocation)?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallStarted {
            tool_call_id: tool_call_id.to_string(),
            model_visible_call_id: None,
            code_mode_runtime_tool_id: None,
            requester: crate::raw_event::RawToolCallRequester::Model,
            kind: ToolCallKind::WriteStdin,
            summary: generic_summary("write_stdin"),
            invocation_payload: Some(invocation_payload.clone()),
        },
    )?;
    let response_payload = writer.write_json_payload(RawPayloadKind::ToolResult, &response)?;
    writer.append_with_context(
        trace_context("turn-1"),
        RawTraceEventPayload::ToolCallEnded {
            tool_call_id: tool_call_id.to_string(),
            status,
            result_payload: Some(response_payload.clone()),
        },
    )?;
    Ok([(invocation_payload, invocation), (response_payload, response)])
}

fn append_inference_with_tool_call(writer: &TraceWriter) -> anyhow::Result<()> {
    let request = writer.write_json_payload(
        RawPayloadKind::InferenceRequest,
        &json!({
            "input": [message("user", "run tests")]
        }),
    )?;
    writer.append(RawTraceEventPayload::InferenceStarted {
        inference_call_id: "inference-1".to_string(),
        thread_id: "thread-root".to_string(),
        codex_turn_id: "turn-1".to_string(),
        model: "gpt-test".to_string(),
        provider_name: "test-provider".to_string(),
        request_payload: request,
    })?;

    let response = writer.write_json_payload(
        RawPayloadKind::InferenceResponse,
        &json!({
            "response_id": "resp-1",
            "output_items": [{
                "type": "function_call",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"cargo test\"}",
                "call_id": "call-1"
            }]
        }),
    )?;
    writer.append(RawTraceEventPayload::InferenceCompleted {
        inference_call_id: "inference-1".to_string(),
        response_id: Some("resp-1".to_string()),
        upstream_request_id: None,
        response_payload: response,
    })?;
    Ok(())
}

fn append_followup_with_tool_output(writer: &TraceWriter) -> anyhow::Result<()> {
    let request = writer.write_json_payload(
        RawPayloadKind::InferenceRequest,
        &json!({
            "previous_response_id": "resp-1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call-1",
                "output": "ok\n"
            }]
        }),
    )?;
    writer.append(RawTraceEventPayload::InferenceStarted {
        inference_call_id: "inference-2".to_string(),
        thread_id: "thread-root".to_string(),
        codex_turn_id: "turn-2".to_string(),
        model: "gpt-test".to_string(),
        provider_name: "test-provider".to_string(),
        request_payload: request,
    })?;
    Ok(())
}
