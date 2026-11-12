//! What changed in a room, for the server to announce.
//!
//! Invariant: a room records an event for each change it makes, oldest first, and the server
//! drains them after each command.

use engine_core::Tick;
use serde::{Deserialize, Serialize};

use super::ids::{PendingId, SeatId, TeamId};

/// Something that changed, for the server to announce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomEvent {
    /// A player or bot took a seat.
    Seated(SeatId),
    /// A joiner is waiting for approval.
    Queued(PendingId),
    /// The host turned a joiner away, or the joiner gave up.
    Withdrawn(PendingId),
    /// A seat was freed by its player (or by a lobby drop).
    Left(SeatId),
    /// The host removed a seat.
    Kicked(SeatId),
    /// A player dropped mid-match; the seat waits for them.
    Disconnected(SeatId),
    /// A dropped player took their seat back.
    Resumed(SeatId),
    /// The host moved to another seat, or there is no host.
    HostChanged(Option<SeatId>),
    /// A seat changed team.
    TeamChanged(SeatId, TeamId),
    /// A seat changed its ready flag.
    ReadyChanged(SeatId, bool),
    /// The host changed the game settings or the admission rule.
    SettingsChanged,
    /// The host started; the match begins at the tick given.
    CountdownStarted(Tick),
    /// The start rule broke (or the host cancelled) during the countdown.
    CountdownCancelled,
    /// The match began.
    MatchStarted,
    /// The match ended.
    MatchEnded,
    /// The room is back in the lobby.
    ReturnedToLobby,
}
