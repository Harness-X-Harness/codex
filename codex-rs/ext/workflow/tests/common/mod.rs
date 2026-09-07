use codex_workflow_extension::ContinuationKind;
use codex_workflow_extension::ContinuationRecord;
use codex_workflow_extension::HostCallResult;
use codex_workflow_extension::request_digest;

pub fn ask_record(instruction: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Ask,
        serde_json::json!({ "instruction": instruction }),
        HostCallResult::success(reply),
    )
}

pub fn agent_record(prompt: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Agent,
        serde_json::json!({ "prompt": prompt, "spawn": false }),
        HostCallResult::success(reply),
    )
}

pub fn agent_failure_record(prompt: &str, error: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Agent,
        serde_json::json!({ "prompt": prompt, "spawn": false }),
        HostCallResult::failure(error),
    )
}

pub fn spawn_record(prompt: &str, task_name: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::SpawnAgent,
        serde_json::json!({
            "prompt": prompt,
            "spawn": true,
            "task_name": task_name,
        }),
        HostCallResult::success(reply),
    )
}

pub fn spawn_failure_record(prompt: &str, task_name: &str, error: &str) -> ContinuationRecord {
    record(
        ContinuationKind::SpawnAgent,
        serde_json::json!({
            "prompt": prompt,
            "spawn": true,
            "task_name": task_name,
        }),
        HostCallResult::failure(error),
    )
}

pub fn pause_record(callsite: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Pause,
        serde_json::json!({ "callsite": callsite }),
        HostCallResult::success(""),
    )
}

pub fn await_user_record(callsite: &str) -> ContinuationRecord {
    record(
        ContinuationKind::AwaitUser,
        serde_json::json!({ "callsite": callsite }),
        HostCallResult::success(""),
    )
}

pub fn source_callsite(source: &str, fn_name: &str, index: usize) -> String {
    let mut seen = 0usize;
    for (line_index, line) in source.lines().enumerate() {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(fn_name) {
            let start = from + rel;
            let end = start + fn_name.len();
            let prev_ok = start == 0 || {
                let prev = line.as_bytes()[start - 1];
                !prev.is_ascii_alphanumeric() && prev != b'_'
            };
            if prev_ok && line[end..].starts_with('(') {
                if seen == index {
                    return format!("{}:{}", line_index + 1, start + 1);
                }
                seen += 1;
            }
            from = start + 1;
        }
    }
    panic!("missing {fn_name}() callsite {index}");
}

fn record(
    kind: ContinuationKind,
    request: serde_json::Value,
    result: HostCallResult,
) -> ContinuationRecord {
    ContinuationRecord {
        seq: 0,
        kind,
        request_digest: request_digest(kind, &request),
        result,
    }
}
