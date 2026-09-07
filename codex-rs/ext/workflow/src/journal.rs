//! Identity-checked continuation journal for `/workflow` resume.

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

use crate::engine::MAX_WORKFLOW_CONTROL_RESUMES;
use crate::engine::MAX_WORKFLOW_YIELDS;

/// Persisted format that stores identity-checked continuation records.
pub const WORKFLOW_PERSIST_VERSION: u32 = 2;

/// Stable fail-closed reason when a journal record does not match the
/// current effective host request.
pub const REPLAY_DIVERGENCE: &str = "workflow replay diverged";

/// Stable reject for active/paused runs that only have positional replies.
pub const LEGACY_RESUME_REQUIRED: &str =
    "workflow persistence requires restart: legacy positional replies cannot be identity-checked";

/// Persisted terminal workflow error when resume identity diverges.
pub const WORKFLOW_ERROR_REPLAY_DIVERGED: &str = "replay_diverged";
/// Persisted terminal workflow error for unsafe legacy positional resume.
pub const WORKFLOW_ERROR_LEGACY_RESUME: &str = "legacy_resume_required";
/// Persisted terminal workflow error for a malformed continuation journal.
pub const WORKFLOW_ERROR_UNSAFE_JOURNAL: &str = "unsafe_journal";
/// Persisted terminal workflow error for an unrecoverable host/runtime fault.
pub const WORKFLOW_ERROR_HOST_RUNTIME: &str = "host_runtime";

/// Secret-safe same-Thread turn failure.
pub const HOST_ERROR_TURN_ERRORED: &str = "turn_errored";
/// Secret-safe same-Thread terminal cancel.
pub const HOST_ERROR_TURN_CANCELLED: &str = "turn_cancelled";
/// Secret-safe stock child error.
pub const HOST_ERROR_CHILD_ERRORED: &str = "child_errored";
/// Secret-safe stock child unavailable/shutdown.
pub const HOST_ERROR_CHILD_UNAVAILABLE: &str = "child_unavailable";

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

/// Codex-native host-call envelope persisted in the continuation journal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostCallResult {
    pub ok: bool,
    pub text: String,
    #[serde(default)]
    pub error: String,
}

impl Default for HostCallResult {
    fn default() -> Self {
        Self::success(String::new())
    }
}

impl HostCallResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            error: String::new(),
        }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: String::new(),
            error: error.into(),
        }
    }
}

fn deserialize_host_call_result<'de, D>(deserializer: D) -> Result<HostCallResult, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(HostCallResult::success(String::new())),
        serde_json::Value::String(text) => Ok(HostCallResult::success(text)),
        serde_json::Value::Object(map) => {
            serde_json::from_value(serde_json::Value::Object(map)).map_err(serde::de::Error::custom)
        }
        other => Err(serde::de::Error::custom(format!(
            "unsupported host call result: {other}"
        ))),
    }
}

/// One dense, ordered continuation record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContinuationRecord {
    pub seq: u32,
    pub kind: ContinuationKind,
    pub request_digest: String,
    #[serde(default, deserialize_with = "deserialize_host_call_result")]
    pub result: HostCallResult,
}

/// Outcome of looking up the next journal record for a host/control call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalLookup {
    Replay(HostCallResult),
    NeedWork,
    Diverged,
}

/// Full SHA-256 hex of the canonical request identity. Not Grok `request_hash`.
pub fn request_digest(kind: ContinuationKind, request: &serde_json::Value) -> String {
    let kind_name = match kind {
        ContinuationKind::Ask => "ask",
        ContinuationKind::Agent => "agent",
        ContinuationKind::SpawnAgent => "spawnAgent",
        ContinuationKind::Pause => "pause",
        ContinuationKind::AwaitUser => "awaitUser",
    };
    let body = serde_json::json!({
        "kind": kind_name,
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

fn is_result_bearing(kind: ContinuationKind) -> bool {
    matches!(
        kind,
        ContinuationKind::Ask | ContinuationKind::Agent | ContinuationKind::SpawnAgent
    )
}

fn is_control_resume(kind: ContinuationKind) -> bool {
    matches!(kind, ContinuationKind::Pause | ContinuationKind::AwaitUser)
}

pub fn result_bearing_count(records: &[ContinuationRecord]) -> u32 {
    u32::try_from(
        records
            .iter()
            .filter(|record| is_result_bearing(record.kind))
            .count(),
    )
    .unwrap_or(u32::MAX)
}

pub fn control_resume_count(records: &[ContinuationRecord]) -> u32 {
    u32::try_from(
        records
            .iter()
            .filter(|record| is_control_resume(record.kind))
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// Fail-closed count caps for eval and restore. Sequence density is
/// persist-owned and is checked by [`bounded`].
pub fn bounded_counts(records: &[ContinuationRecord]) -> Result<(), String> {
    if result_bearing_count(records) > MAX_WORKFLOW_YIELDS {
        return Err(format!("workflow exceeded {MAX_WORKFLOW_YIELDS} yields"));
    }
    if control_resume_count(records) > MAX_WORKFLOW_CONTROL_RESUMES {
        return Err(format!(
            "workflow exceeded {MAX_WORKFLOW_CONTROL_RESUMES} control resumes"
        ));
    }
    Ok(())
}

pub fn bounded(records: &[ContinuationRecord]) -> Result<(), String> {
    bounded_counts(records)?;
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
