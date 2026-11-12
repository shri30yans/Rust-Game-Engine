//! What clients see of a room: the seat views and the room view, with no resume tokens.
//!
//! Invariant: a view never carries a resume token. [`Room::view`] is the only way to build one.

use serde::{Deserialize, Serialize};

use super::config::RoomConfig;
use super::ids::{PendingId, SeatId, TeamId};
use super::phase::Phase;
use super::seat::Occupant;
use super::state::Room;
use crate::name::PlayerName;

/// One seat as clients see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatView {
    /// The seat's id.
    pub id: SeatId,
    /// The display name.
    pub name: PlayerName,
    /// The team.
    pub team: TeamId,
    /// Whether the seat is ready.
    pub ready: bool,
    /// Human (and whether connected) or bot.
    pub occupant: Occupant,
}

/// The whole room as clients see it: no tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomView<S> {
    /// The rules.
    pub config: RoomConfig,
    /// The game's settings.
    pub settings: S,
    /// Where the room is in its cycle.
    pub phase: Phase,
    /// The host's seat.
    pub host: Option<SeatId>,
    /// Every filled seat, in seat order.
    pub seats: Vec<SeatView>,
    /// Joiners waiting for approval.
    pub waiting: Vec<(PendingId, PlayerName)>,
}

impl<S> Room<S> {
    /// The room as clients see it.
    #[must_use]
    pub fn view(&self) -> RoomView<S>
    where
        S: Clone,
    {
        RoomView {
            config: self.config,
            settings: self.settings.clone(),
            phase: self.phase,
            host: self.host,
            seats: self
                .seats
                .iter()
                .map(|seat| SeatView {
                    id: seat.id,
                    name: seat.name.clone(),
                    team: seat.team,
                    ready: seat.ready,
                    occupant: seat.occupant(),
                })
                .collect(),
            waiting: self
                .pending
                .iter()
                .map(|pending| (pending.id, pending.name.clone()))
                .collect(),
        }
    }
}
