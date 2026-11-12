//! A filled seat: who sits in it, its team, its ready flag and the human's resume token.
//!
//! Invariant: the resume token stays inside the seat. [`Seat::occupant`] reveals only whether
//! the seat is a human, and whether that human is connected, never the token. A bot is always
//! ready.

use serde::{Deserialize, Serialize};

use super::ids::{SeatId, TeamId};
use crate::codes::ResumeToken;
use crate::name::PlayerName;

/// Who sits in a seat, as anyone may see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Occupant {
    /// A person; `connected` is false while they are dropped mid-match.
    Human {
        /// Whether their connection is live.
        connected: bool,
    },
    /// A server-controlled player.
    Bot,
}

/// Who sits in a seat, including the human's secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Human { token: ResumeToken, connected: bool },
    Bot,
}

/// One filled seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub(super) id: SeatId,
    pub(super) name: PlayerName,
    pub(super) team: TeamId,
    pub(super) ready: bool,
    pub(super) kind: Kind,
}

impl Seat {
    /// The seat's id.
    #[must_use]
    pub const fn id(&self) -> SeatId {
        self.id
    }

    /// The display name.
    #[must_use]
    pub const fn name(&self) -> &PlayerName {
        &self.name
    }

    /// The seat's team.
    #[must_use]
    pub const fn team(&self) -> TeamId {
        self.team
    }

    /// Whether the seat is ready to start (bots always are).
    #[must_use]
    pub const fn ready(&self) -> bool {
        self.ready
    }

    /// Human or bot, without the token.
    #[must_use]
    pub const fn occupant(&self) -> Occupant {
        match self.kind {
            Kind::Human { connected, .. } => Occupant::Human { connected },
            Kind::Bot => Occupant::Bot,
        }
    }

    pub(super) const fn is_human(&self) -> bool {
        matches!(self.kind, Kind::Human { .. })
    }

    pub(super) const fn is_connected_human(&self) -> bool {
        matches!(
            self.kind,
            Kind::Human {
                connected: true,
                ..
            }
        )
    }

    pub(super) fn has_token(&self, token: &ResumeToken) -> bool {
        matches!(&self.kind, Kind::Human { token: own, .. } if own == token)
    }
}
