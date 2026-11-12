//! Identifiers a room issues: seats, queued joiners and teams.
//!
//! Invariants: a room hands out each [`SeatId`] and [`PendingId`] once and never reuses it, so
//! an id a client holds cannot come to mean a different player. A [`TeamId`] is checked
//! against the room's team count by the room that receives it.

use serde::{Deserialize, Serialize};

/// A seat's id, unique within its room for the room's whole life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SeatId(pub(super) u32);

impl SeatId {
    /// Builds an identifier from its wire value. Rooms issue identifiers from their own sequence;
    /// callers use this for cross-layer player identity values.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A queued joiner's id, unique within its room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PendingId(pub(super) u32);

/// A team index, below the room's team count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamId(pub(super) u8);

impl TeamId {
    /// A team id; whether it exists is checked by the room that receives it.
    #[must_use]
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    /// The team index.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }
}
