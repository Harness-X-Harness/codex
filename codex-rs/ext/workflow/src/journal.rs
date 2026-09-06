//! Identity-checked continuation journal for `/workflow` resume.

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

use crate::engine::MAX_WORKFLOW_YIELDS;

/// Persisted format that stores identity-checked continuation records.
pub const WORKFLOW_PERSIST_VERSION: u32 = 2;

/// Stable fail-closed reason when a journal record does not match the
/// current effective host request.
pub const REPLAY_DIVERGENCE: &str = "workflow replay diverged";

/// Stable reject for active/paused runs that only have positional replies.
pub const LEGACY_RESUME_REQUIRED: &str =
    "workflow persistence requires restart: legacy positional replies cannot be identity-checked";

/// Kind of one host or control continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContinuationKind {
    Ask,
    Agent,
    SpawnAgent,
    Pause,
    AwaitUser,
}

/// One dense, ordered continuation record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContinuationRecord {
    pub seq: u32,
    pub kind: ContinuationKind,
    pub request_digest: String,
    #[serde(default)]
    pub result: String,
}

/// Outcome of looking up the next journal record for a host/control call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalLookup {
    Replay(String),
    NeedWork,
    Diverged,
}

impl ContinuationKind {
    fn wire_name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Agent => "agent",
            Self::SpawnAgent => "spawnAgent",
            Self::Pause => "pause",
            Self::AwaitUser => "awaitUser",
        }
    }
}

/// Full SHA-256 hex of the canonical request identity. Not Grok `request_hash`.
pub fn request_digest(kind: ContinuationKind, request: &serde_json::Value) -> String {
    let body = serde_json::json!({
        "kind": kind.wire_name(),
        "request": request,
    });
    let encoded = serde_json::to_vec(&body).unwrap_or_else(|_| Vec::new());
    format!("{:x}", Sha256::digest(encoded))
}

pub fn ask_request(instruction: &str) -> serde_json::Value {
    serde_json::json!({ "instruction": instruction })
}

pub fn agent_request(prompt: &str) -> serde_json::Value {
    serde_json::json!({ "prompt": prompt, "spawn": false })
}

pub fn spawn_request(prompt: &str, task_name: &str) -> serde_json::Value {
    serde_json::json!({
        "prompt": prompt,
        "spawn": true,
        "task_name": task_name,
    })
}

pub fn control_request() -> serde_json::Value {
    serde_json::json!({})
}

pub fn lookup(
    records: &[ContinuationRecord],
    index: usize,
    kind: ContinuationKind,
    digest: &str,
) -> JournalLookup {
    match records.get(index) {
        None => JournalLookup::NeedWork,
        Some(record) if record.kind == kind && record.request_digest == digest => {
            JournalLookup::Replay(record.result.clone())
        }
        Some(_) => JournalLookup::Diverged,
    }
}

pub fn next_seq(records: &[ContinuationRecord]) -> u32 {
    u32::try_from(records.len().saturating_add(1)).unwrap_or(u32::MAX)
}

pub fn result_bearing_count(records: &[ContinuationRecord]) -> u32 {
    u32::try_from(
        records
            .iter()
            .filter(|record| {
                matches!(
                    record.kind,
                    ContinuationKind::Ask | ContinuationKind::Agent | ContinuationKind::SpawnAgent
                )
            })
            .count(),
    )
    .unwrap_or(u32::MAX)
}

pub fn bounded(records: &[ContinuationRecord]) -> Result<(), String> {
    if records.len() > MAX_WORKFLOW_YIELDS as usize {
        return Err(format!("workflow exceeded {MAX_WORKFLOW_YIELDS} yields"));
    }
    for (index, record) in records.iter().enumerate() {
        let expected = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        if record.seq != expected {
            return Err("workflow continuation sequence is not dense".to_string());
        }
        if record.request_digest.is_empty() {
            return Err("workflow continuation is missing a request digest".to_string());
        }
    }
    Ok(())
}

pub fn ask_record(instruction: &str, reply: &str) -> ContinuationRecord {
    record(ContinuationKind::Ask, &ask_request(instruction), reply)
}

pub fn agent_record(prompt: &str, reply: &str) -> ContinuationRecord {
    record(ContinuationKind::Agent, &agent_request(prompt), reply)
}

pub fn spawn_record(prompt: &str, task_name: &str, reply: &str) -> ContinuationRecord {
    record(
        ContinuationKind::SpawnAgent,
        &spawn_request(prompt, task_name),
        reply,
    )
}

pub fn pause_record() -> ContinuationRecord {
    record(ContinuationKind::Pause, &control_request(), "")
}

pub fn await_user_record() -> ContinuationRecord {
    record(ContinuationKind::AwaitUser, &control_request(), "")
}

fn record(kind: ContinuationKind, request: &serde_json::Value, result: &str) -> ContinuationRecord {
    ContinuationRecord {
        seq: 0,
        kind,
        request_digest: request_digest(kind, request),
        result: result.to_string(),
    }
}
