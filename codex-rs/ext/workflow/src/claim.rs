//! Transactional `/workflow` engine-slot claims.

use std::sync::Arc;

use codex_extension_api::EngineOccupant;
use codex_extension_api::EngineSlot;

use crate::run::WorkflowRun;

/// Result of reconciling a persisted run with the in-memory engine slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipEffect {
    Held,
    Parked,
    Released { kicked: bool },
}

/// Newly acquired Workflow occupancy that rolls back unless [`commit`](Self::commit)d.
pub struct WorkflowClaim {
    slot: Option<Arc<EngineSlot>>,
    succeeded: bool,
    release_on_drop: bool,
}

impl WorkflowClaim {
    /// Claims `slot` when present. A missing slot is a vacuous success with
    /// nothing to roll back (no live Thread).
    pub fn acquire(slot: Option<Arc<EngineSlot>>) -> Self {
        let Some(slot) = slot else {
            return Self {
                slot: None,
                succeeded: true,
                release_on_drop: false,
            };
        };
        let vacant = slot.occupant().is_none();
        if !slot.try_claim(EngineOccupant::Workflow) {
            return Self {
                slot: None,
                succeeded: false,
                release_on_drop: false,
            };
        }
        Self {
            slot: Some(slot),
            succeeded: true,
            release_on_drop: vacant,
        }
    }

    /// Start paths treat a successful reclaim as rollback-eligible so a
    /// phantom Workflow occupant cannot survive a failed start.
    pub fn rollback_if_held(&mut self) {
        if self.succeeded {
            self.release_on_drop = true;
        }
    }

    pub fn succeeded(&self) -> bool {
        self.succeeded
    }

    pub fn commit(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for WorkflowClaim {
    fn drop(&mut self) {
        if !self.release_on_drop {
            return;
        }
        if let Some(slot) = &self.slot {
            let _ = slot.release(EngineOccupant::Workflow);
        }
    }
}

/// Align persisted run status with the live engine slot.
///
/// `HostIdleHold` is derived from [`OwnershipEffect::Held`] only.
pub fn reconcile_workflow_ownership(slot: &EngineSlot, run: &mut WorkflowRun) -> OwnershipEffect {
    if run.occupies_idle() {
        if slot.try_claim(EngineOccupant::Workflow) {
            OwnershipEffect::Held
        } else if run.yield_occupancy().is_ok() {
            OwnershipEffect::Parked
        } else {
            OwnershipEffect::Released { kicked: false }
        }
    } else {
        OwnershipEffect::Released {
            kicked: slot.release(EngineOccupant::Workflow),
        }
    }
}
