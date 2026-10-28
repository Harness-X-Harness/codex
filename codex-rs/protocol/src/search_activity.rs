//! Transient search observations. Canonical items alone own history and results.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SearchActivityKind {
    Web,
    X,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SearchActivityState {
    Running,
    /// A validated completed item arrived; canonical retention may still fail.
    Completed,
    /// The attempt ended without retaining this observation's canonical item.
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS, JsonSchema)]
pub struct SearchActivityEvent {
    /// Core-generated, increasing within this server process; never provider supplied.
    #[ts(type = "number")]
    pub attempt_id: u64,
    #[ts(type = "number")]
    pub output_index: u64,
    pub item_id: String,
    pub kind: SearchActivityKind,
    pub state: SearchActivityState,
}

#[cfg(test)]
#[path = "search_activity_tests.rs"]
mod tests;
