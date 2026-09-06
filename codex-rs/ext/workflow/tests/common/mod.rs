use codex_workflow_extension::ContinuationKind;
use codex_workflow_extension::ContinuationRecord;
use codex_workflow_extension::request_digest;

pub fn ask_record(instruction: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Ask,
        serde_json::json!({ "instruction": instruction }),
        reply,
    )
}

pub fn agent_record(prompt: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::Agent,
        serde_json::json!({ "prompt": prompt, "spawn": false }),
        reply,
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
        reply,
    )
}

pub fn pause_record() -> ContinuationRecord {
    record(ContinuationKind::Pause, serde_json::json!({}), "")
}

pub fn await_user_record() -> ContinuationRecord {
    record(ContinuationKind::AwaitUser, serde_json::json!({}), "")
}

fn record(kind: ContinuationKind, request: serde_json::Value, result: &str) -> ContinuationRecord {
    ContinuationRecord {
        seq: 0,
        kind,
        request_digest: request_digest(kind, &request),
        result: result.to_string(),
    }
}
