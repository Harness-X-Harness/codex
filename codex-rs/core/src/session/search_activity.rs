//! One sampling attempt owns transient activity and clears it before any retry or
//! tool drain. The sequential response loop is the only producer of this scope.

use super::session::Session;
use super::turn_context::TurnContext;
use codex_protocol::SearchActivityEvent;
use codex_protocol::SearchActivityKind;
use codex_protocol::SearchActivityState;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::EventMsg;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

const MAX_ACTIVITIES: usize = 65; // 64 buffered indexes plus the released open head.
const MAX_ITEM_ID_BYTES: usize = 1024;
// At most 256 KiB of identifier bytes, independent of model-history truncation.
const MAX_TURN_IDENTITIES: usize = 256;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);

pub(crate) struct SearchActivityScope {
    attempt_id: u64,
    pending: BTreeMap<u64, (SearchActivityKind, String, SearchActivityState)>,
}

impl SearchActivityScope {
    pub(crate) fn new() -> CodexResult<Self> {
        let attempt_id = NEXT_ATTEMPT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                (id < MAX_SAFE_INTEGER).then_some(id + 1)
            })
            .map_err(|_| CodexErr::Stream("search attempt identity exhausted".into()))?;
        Ok(Self {
            attempt_id,
            pending: BTreeMap::new(),
        })
    }

    pub(super) async fn observe(
        &mut self,
        sess: &Session,
        turn: &TurnContext,
        output_index: u64,
        item_id: String,
        kind: SearchActivityKind,
        state: SearchActivityState,
    ) -> CodexResult<()> {
        let valid = !item_id.is_empty()
            && item_id.len() <= MAX_ITEM_ID_BYTES
            && output_index <= MAX_SAFE_INTEGER;
        let changed = match (state, self.pending.get(&output_index)) {
            (SearchActivityState::Running, None)
                if valid
                    && self.pending.len() < MAX_ACTIVITIES
                    && !self.pending.values().any(|(_, id, _)| id == &item_id) =>
            {
                true
            }
            (
                SearchActivityState::Completed,
                Some((current_kind, id, SearchActivityState::Running)),
            ) if id == &item_id && *current_kind == kind => true,
            (state, Some((current_kind, id, current)))
                if id == &item_id && *current_kind == kind && *current == state =>
            {
                false
            }
            _ => return Err(CodexErr::Stream("invalid search activity lifetime".into())),
        };
        if changed {
            if state == SearchActivityState::Running {
                identity_fence(turn).reserve_search(&item_id, self.attempt_id)?;
            }
            self.pending
                .insert(output_index, (kind, item_id.clone(), state));
            sess.send_event(
                turn,
                EventMsg::SearchActivity(SearchActivityEvent {
                    attempt_id: self.attempt_id,
                    output_index,
                    item_id,
                    kind,
                    state,
                }),
            )
            .await;
        }
        Ok(())
    }

    /// Contributors may decorate items, but cannot steal or rebind an outstanding
    /// preview. Also catches an earlier message rewritten as another live Web ID.
    pub(crate) fn validate_canonical(
        &self,
        turn: &TurnContext,
        original: &codex_protocol::models::ResponseItem,
        finalized: Option<&codex_protocol::items::TurnItem>,
    ) -> CodexResult<()> {
        use codex_protocol::items::TurnItem;
        use codex_protocol::models::ResponseItem;
        let original_id = match original {
            ResponseItem::WebSearchCall { id: Some(id), .. } => Some(id.as_str()),
            _ => None,
        };
        let canonical_id = match finalized {
            Some(TurnItem::WebSearch(item)) => Some(item.id.as_str()),
            Some(TurnItem::Extension(codex_extension_items::ExtensionItem::WebSearch(item))) => {
                Some(item.id.as_str())
            }
            _ => None,
        };
        let touches_preview = self.pending.values().any(|(_, id, _)| {
            Some(id.as_str()) == original_id || Some(id.as_str()) == canonical_id
        });
        if touches_preview && original_id != canonical_id {
            return Err(CodexErr::Stream(
                "canonical item conflicts with an outstanding search activity".into(),
            ));
        }
        if let Some(finalized) = finalized {
            let id = finalized.id();
            let owns_search = self.pending.values().any(|(_, item_id, _)| item_id == &id)
                && original_id == Some(id.as_str())
                && canonical_id == Some(id.as_str());
            identity_fence(turn)
                .validate_and_record(&id, owns_search.then_some(self.attempt_id))?;
        }
        Ok(())
    }

    /// Reject later raw rebinding before any started item can replace retained Web history.
    pub(super) fn validate_response_identity(
        &self,
        turn: &TurnContext,
        item: &codex_protocol::models::ResponseItem,
    ) -> CodexResult<()> {
        if let Some(id) = item.id() {
            identity_fence(turn).reject_retained_search(id.as_str())?;
        }
        Ok(())
    }

    /// Claim a known ordinary identity before asynchronous work can publish it.
    /// Local IDs can come from call_id; Plan mode has a turn-derived generated ID.
    pub(crate) fn claim_canonical_identity(
        &self,
        turn: &TurnContext,
        call_id: &str,
    ) -> CodexResult<()> {
        identity_fence(turn).validate_and_record(call_id, /*owner*/ None)
    }

    /// Called only after the ordered canonical item has been successfully handled.
    /// Simultaneously live IDs are unique at ingress. The response loop awaits
    /// canonical delivery before reading another observation. Retained identities
    /// stay reserved for this turn even after this attempt's pending entry is evicted.
    pub(super) fn retained(&mut self, turn: &TurnContext, item_id: &str) {
        identity_fence(turn).retain_search(item_id, self.attempt_id);
        self.pending.retain(|_, (_, id, _)| id != item_id);
    }

    pub(super) async fn clear(&mut self, sess: &Session, turn: &TurnContext) {
        for (output_index, (kind, item_id, _)) in std::mem::take(&mut self.pending) {
            identity_fence(turn).abandon_search(&item_id, self.attempt_id);
            sess.send_event(
                turn,
                EventMsg::SearchActivity(SearchActivityEvent {
                    attempt_id: self.attempt_id,
                    output_index,
                    item_id,
                    kind,
                    state: SearchActivityState::Cleared,
                }),
            )
            .await;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CanonicalIdentity {
    Other,
    PendingSearch(u64),
    RetainedSearch,
}

#[derive(Default)]
struct TurnIdentities {
    ids: HashMap<String, CanonicalIdentity>,
    // Once an ordinary identity could not be tracked, forgetting that fact could
    // admit a later hosted collision after compaction. Ordinary work still runs.
    saturated: bool,
}

#[derive(Default)]
struct SearchIdentityFence(Mutex<TurnIdentities>);

fn identity_fence(turn: &TurnContext) -> Arc<SearchIdentityFence> {
    turn.extension_data
        .get_or_init(SearchIdentityFence::default)
}

impl SearchIdentityFence {
    fn reserve_search(&self, id: &str, attempt_id: u64) -> CodexResult<()> {
        if id.is_empty() || id.len() > MAX_ITEM_ID_BYTES {
            return Err(CodexErr::Stream(
                "invalid hosted search item ID length".into(),
            ));
        }
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.ids.contains_key(id) {
            return Err(CodexErr::Stream(
                "search item ID is already used in this turn".into(),
            ));
        }
        if state.saturated || state.ids.len() >= MAX_TURN_IDENTITIES {
            return Err(CodexErr::Stream(
                "turn search identity tracking limit exceeded".into(),
            ));
        }
        state
            .ids
            .insert(id.to_owned(), CanonicalIdentity::PendingSearch(attempt_id));
        Ok(())
    }

    fn reject_retained_search(&self, id: &str) -> CodexResult<()> {
        if self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ids
            .get(id)
            == Some(&CanonicalIdentity::RetainedSearch)
        {
            return Err(CodexErr::Stream(
                "retained search item ID cannot be rebound in this turn".into(),
            ));
        }
        Ok(())
    }

    fn validate_and_record(&self, id: &str, owner: Option<u64>) -> CodexResult<()> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match state.ids.get(id) {
            Some(CanonicalIdentity::RetainedSearch) => {
                return Err(CodexErr::Stream(
                    "retained search item ID cannot be rebound in this turn".into(),
                ));
            }
            Some(CanonicalIdentity::PendingSearch(attempt_id)) if owner != Some(*attempt_id) => {
                return Err(CodexErr::Stream(
                    "canonical item conflicts with a pending search identity".into(),
                ));
            }
            _ => {}
        }
        state.record_other(id);
        Ok(())
    }

    fn retain_search(&self, id: &str, attempt_id: u64) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.ids.get(id) == Some(&CanonicalIdentity::PendingSearch(attempt_id)) {
            state
                .ids
                .insert(id.to_owned(), CanonicalIdentity::RetainedSearch);
        }
    }

    fn abandon_search(&self, id: &str, attempt_id: u64) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.ids.get(id) == Some(&CanonicalIdentity::PendingSearch(attempt_id)) {
            state.ids.remove(id);
        }
    }
}

impl TurnIdentities {
    fn record_other(&mut self, id: &str) {
        // An ordinary ID outside the supported hosted-ID range cannot collide.
        if id.is_empty() || id.len() > MAX_ITEM_ID_BYTES || self.ids.contains_key(id) {
            return;
        }
        if self.ids.len() == MAX_TURN_IDENTITIES {
            self.saturated = true;
        } else {
            self.ids.insert(id.to_owned(), CanonicalIdentity::Other);
        }
    }
}

/// Record-only hook: existing ordinary item behavior is unchanged, including
/// repeated local IDs. Saturation only blocks subsequent hosted admission.
pub(crate) fn record_canonical_identity(
    turn: &TurnContext,
    item: &codex_protocol::items::TurnItem,
) {
    identity_fence(turn)
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record_other(&item.id());
}

#[cfg(test)]
#[path = "search_activity_tests.rs"]
mod tests;
