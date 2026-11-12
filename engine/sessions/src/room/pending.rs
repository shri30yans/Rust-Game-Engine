//! Queued joiners: players who asked to join a room with approval admission and wait for the
//! host.
//!
//! Invariant: a queued joiner holds a resume token of its own and sits in no seat until the
//! host approves it.

use super::ids::{PendingId, SeatId};
use crate::codes::ResumeToken;
use crate::name::PlayerName;

/// A joiner waiting for the host's approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pending {
    pub(super) id: PendingId,
    pub(super) name: PlayerName,
    pub(super) token: ResumeToken,
}

/// What a join did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinOutcome {
    /// The player has a seat.
    Seated(SeatId),
    /// The player waits for the host's approval.
    Queued(PendingId),
}
