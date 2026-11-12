//! The room's state: its rules, settings, phase, seats, queue and host, with the lookups over
//! them.
//!
//! Invariant: after every command, `settle` restores the host rule (a host is a connected human
//! whenever one is seated) and the countdown rule (a countdown has a host and meets the start
//! rule).

use crate::codes::ResumeToken;
use crate::error::SessionError;

use super::config::RoomConfig;
use super::event::RoomEvent;
use super::ids::{PendingId, SeatId, TeamId};
use super::pending::Pending;
use super::phase::Phase;
use super::seat::Seat;

/// One game session: seats, host, admission and the match cycle. `S` is the game's settings.
#[derive(Debug, Clone)]
pub struct Room<S> {
    pub(super) config: RoomConfig,
    pub(super) settings: S,
    pub(super) phase: Phase,
    pub(super) seats: Vec<Seat>,
    pub(super) pending: Vec<Pending>,
    pub(super) host: Option<SeatId>,
    pub(super) next_seat: u32,
    pub(super) next_pending: u32,
    pub(super) events: Vec<RoomEvent>,
}

impl<S> Room<S> {
    /// An empty room in the lobby.
    #[must_use]
    pub fn new(config: RoomConfig, settings: S) -> Self {
        let seats = usize::from(config.max_seats());
        Self {
            config,
            settings,
            phase: Phase::Lobby,
            seats: Vec::with_capacity(seats),
            pending: Vec::with_capacity(seats),
            host: None,
            next_seat: 0,
            next_pending: 0,
            events: Vec::with_capacity(16),
        }
    }

    /// The rules.
    #[must_use]
    pub const fn config(&self) -> &RoomConfig {
        &self.config
    }

    /// The game's settings.
    #[must_use]
    pub const fn settings(&self) -> &S {
        &self.settings
    }

    /// Where the room is in its cycle.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The host's seat; `None` exactly when no human is seated.
    #[must_use]
    pub const fn host(&self) -> Option<SeatId> {
        self.host
    }

    /// Every filled seat, in seat order.
    #[must_use]
    pub fn seats(&self) -> &[Seat] {
        &self.seats
    }

    /// One seat.
    #[must_use]
    pub fn seat(&self, id: SeatId) -> Option<&Seat> {
        self.seats.iter().find(|seat| seat.id == id)
    }

    /// The seat a resume token owns.
    #[must_use]
    pub fn seat_of(&self, token: &ResumeToken) -> Option<SeatId> {
        self.seats
            .iter()
            .find(|seat| seat.has_token(token))
            .map(Seat::id)
    }

    /// Whether the start rule holds: enough seats filled and every human ready.
    #[must_use]
    pub fn can_start(&self) -> bool {
        self.seats.len() >= usize::from(self.config.min_to_start())
            && self.seats.iter().all(|seat| seat.ready)
    }

    /// Hands out the events recorded since the last drain, oldest first.
    pub fn drain_events(&mut self) -> std::vec::Drain<'_, RoomEvent> {
        self.events.drain(..)
    }

    pub(super) fn is_full(&self) -> bool {
        self.seats.len() >= usize::from(self.config.max_seats())
    }

    pub(super) fn has_connected_host(&self) -> bool {
        self.host
            .and_then(|host| self.seat(host))
            .is_some_and(Seat::is_connected_human)
    }

    pub(super) fn team_count(&self, team: TeamId) -> usize {
        self.seats.iter().filter(|seat| seat.team == team).count()
    }

    pub(super) fn index_of(&self, id: SeatId) -> Result<usize, SessionError> {
        self.seats
            .iter()
            .position(|seat| seat.id == id)
            .ok_or(SessionError::UnknownSeat)
    }

    pub(super) fn pending_index(&self, id: PendingId) -> Result<usize, SessionError> {
        self.pending
            .iter()
            .position(|pending| pending.id == id)
            .ok_or(SessionError::UnknownPending)
    }

    pub(super) fn require_host(&self, by: SeatId) -> Result<(), SessionError> {
        if self.host == Some(by) {
            Ok(())
        } else {
            Err(SessionError::NotHost)
        }
    }

    pub(super) fn require_before_match(&self) -> Result<(), SessionError> {
        match self.phase {
            Phase::Lobby | Phase::Countdown { .. } => Ok(()),
            Phase::InMatch | Phase::Results => Err(SessionError::WrongPhase),
        }
    }

    /// Restores the host and countdown invariants after any change to the seats.
    pub(super) fn settle(&mut self) {
        let keep = self
            .host
            .and_then(|host| self.seat(host))
            .is_some_and(|seat| {
                seat.is_connected_human() || !self.seats.iter().any(Seat::is_connected_human)
            });
        if !keep {
            let host = self
                .seats
                .iter()
                .find(|seat| seat.is_connected_human())
                .or_else(|| self.seats.iter().find(|seat| seat.is_human()))
                .map(Seat::id);
            if host != self.host {
                self.host = host;
                self.events.push(RoomEvent::HostChanged(host));
            }
        }
        if matches!(self.phase, Phase::Countdown { .. })
            && (self.host.is_none() || !self.can_start())
        {
            self.phase = Phase::Lobby;
            self.events.push(RoomEvent::CountdownCancelled);
        }
    }
}
