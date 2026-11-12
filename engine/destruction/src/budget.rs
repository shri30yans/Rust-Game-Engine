//! Per-step work budget for the solver.
//!
//! The budget counts element visits. One visit is one element processed in one
//! solver phase, so the cost of a step is bounded by construction. Work that
//! does not fit carries over to the next step, in the same order.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// Maximum element visits one [`StructureState::step`](crate::state::StructureState::step) may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestructionBudget {
    max_element_visits: NonZeroU32,
}

impl DestructionBudget {
    /// Creates a budget. A zero budget could never make progress, so the type rules it out.
    #[must_use]
    pub const fn new(max_element_visits: NonZeroU32) -> Self {
        Self { max_element_visits }
    }

    /// Visits one step may spend.
    #[must_use]
    pub const fn max_element_visits(self) -> NonZeroU32 {
        self.max_element_visits
    }
}
