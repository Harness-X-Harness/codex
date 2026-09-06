//! One Thread admits at most one `active` Goal HOW or `/workflow` occupant.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use crate::ExtensionData;

/// Who currently occupies the host Goal / workflow engine slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineOccupant {
    GoalHow,
    Workflow,
}

/// In-memory engine slot. Persist decides who is waiting; this slot decides
/// who may run host-owned continuation now.
#[derive(Debug, Default)]
pub struct EngineSlot {
    occupant: Mutex<Option<EngineOccupant>>,
}

impl EngineSlot {
    /// Claims the slot for `who`. Succeeds when the slot is empty or already
    /// held by the same occupant.
    pub fn try_claim(&self, who: EngineOccupant) -> bool {
        let mut occupant = self.occupant.lock().unwrap_or_else(PoisonError::into_inner);
        match *occupant {
            None => {
                *occupant = Some(who);
                true
            }
            Some(current) => current == who,
        }
    }

    /// Releases the slot when `who` currently occupies it.
    pub fn release(&self, who: EngineOccupant) -> bool {
        let mut occupant = self.occupant.lock().unwrap_or_else(PoisonError::into_inner);
        if *occupant == Some(who) {
            *occupant = None;
            true
        } else {
            false
        }
    }

    pub fn occupant(&self) -> Option<EngineOccupant> {
        *self.occupant.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Returns the thread's engine slot, creating an empty one when absent.
pub fn engine_slot(data: &ExtensionData) -> Arc<EngineSlot> {
    data.get_or_init(EngineSlot::default)
}
