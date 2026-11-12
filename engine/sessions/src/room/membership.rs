//! Who is in the room: joins, resumes, drops, leaving, the host approving or removing joiners,
//! and bots.
//!
//! Invariant: a seat is taken only below `max_seats` and in a team with a free slot. Every
//! change to the seats ends in `settle`, so the host and countdown rules hold afterwards.

use crate::codes::ResumeToken;
use crate::error::SessionError;
use crate::name::PlayerName;

use super::config::Admission;
use super::event::RoomEvent;
use super::ids::{PendingId, SeatId, TeamId};
use super::pending::{JoinOutcome, Pending};
use super::phase::Phase;
use super::seat::{Kind, Seat};
use super::state::Room;

impl<S> Room<S> {
    /// A player asks to join with a fresh token.
    ///
    /// # Errors
    /// [`SessionError::AlreadyInRoom`] when the token is known (use [`Room::resume`]),
    /// [`SessionError::Closed`], [`SessionError::RoomFull`] or [`SessionError::QueueFull`].
    pub fn join(
        &mut self,
        name: PlayerName,
        token: ResumeToken,
    ) -> Result<JoinOutcome, SessionError> {
        let known = self.seats.iter().any(|seat| seat.has_token(&token))
            || self.pending.iter().any(|pending| pending.token == token);
        if known {
            return Err(SessionError::AlreadyInRoom);
        }
        match self.config.admission() {
            Admission::Closed => Err(SessionError::Closed),
            Admission::Approval if self.has_connected_host() => {
                if self.is_full() {
                    return Err(SessionError::RoomFull);
                }
                if self.pending.len() >= usize::from(self.config.max_seats()) {
                    return Err(SessionError::QueueFull);
                }
                let id = PendingId(self.next_pending);
                self.next_pending = self.next_pending.wrapping_add(1);
                self.pending.push(Pending { id, name, token });
                self.events.push(RoomEvent::Queued(id));
                Ok(JoinOutcome::Queued(id))
            }
            Admission::Open | Admission::Approval => self
                .seat_new(
                    name,
                    Kind::Human {
                        token,
                        connected: true,
                    },
                )
                .map(JoinOutcome::Seated),
        }
    }

    /// A dropped player takes their seat back.
    ///
    /// # Errors
    /// [`SessionError::UnknownToken`] when no seat has the token.
    pub fn resume(&mut self, token: &ResumeToken) -> Result<SeatId, SessionError> {
        let seat = self
            .seats
            .iter_mut()
            .find(|seat| seat.has_token(token))
            .ok_or(SessionError::UnknownToken)?;
        if let Kind::Human { connected, .. } = &mut seat.kind {
            *connected = true;
        }
        let id = seat.id;
        self.events.push(RoomEvent::Resumed(id));
        self.settle();
        Ok(id)
    }

    /// A human's connection dropped. In the lobby the seat is freed; from the match on it is
    /// kept for [`Room::resume`].
    ///
    /// # Errors
    /// [`SessionError::UnknownSeat`] or, for a bot, [`SessionError::WrongOccupant`].
    pub fn disconnect(&mut self, id: SeatId) -> Result<(), SessionError> {
        let index = self.index_of(id)?;
        match self.phase {
            Phase::Lobby | Phase::Countdown { .. } => {
                if !self.seats.get(index).is_some_and(Seat::is_human) {
                    return Err(SessionError::WrongOccupant);
                }
                self.seats.remove(index);
                self.events.push(RoomEvent::Left(id));
            }
            Phase::InMatch | Phase::Results => {
                let seat = self.seats.get_mut(index).ok_or(SessionError::UnknownSeat)?;
                let Kind::Human { connected, .. } = &mut seat.kind else {
                    return Err(SessionError::WrongOccupant);
                };
                *connected = false;
                self.events.push(RoomEvent::Disconnected(id));
            }
        }
        self.settle();
        Ok(())
    }

    /// A player gives up their seat, in any phase.
    ///
    /// # Errors
    /// [`SessionError::UnknownSeat`] or, for a bot, [`SessionError::WrongOccupant`].
    pub fn leave(&mut self, id: SeatId) -> Result<(), SessionError> {
        let index = self.index_of(id)?;
        if !self.seats.get(index).is_some_and(Seat::is_human) {
            return Err(SessionError::WrongOccupant);
        }
        self.seats.remove(index);
        self.events.push(RoomEvent::Left(id));
        self.settle();
        Ok(())
    }

    /// A queued joiner gives up (their connection dropped or they cancelled).
    ///
    /// # Errors
    /// [`SessionError::UnknownPending`].
    pub fn withdraw(&mut self, id: PendingId) -> Result<(), SessionError> {
        let index = self.pending_index(id)?;
        self.pending.remove(index);
        self.events.push(RoomEvent::Withdrawn(id));
        Ok(())
    }

    /// The host seats a queued joiner.
    ///
    /// # Errors
    /// [`SessionError::NotHost`], [`SessionError::UnknownPending`] or
    /// [`SessionError::RoomFull`] (the joiner stays queued).
    pub fn approve(&mut self, by: SeatId, id: PendingId) -> Result<SeatId, SessionError> {
        self.require_host(by)?;
        let index = self.pending_index(id)?;
        if self.is_full() {
            return Err(SessionError::RoomFull);
        }
        let pending = self.pending.remove(index);
        self.seat_new(
            pending.name,
            Kind::Human {
                token: pending.token,
                connected: true,
            },
        )
    }

    /// The host turns a queued joiner away.
    ///
    /// # Errors
    /// [`SessionError::NotHost`] or [`SessionError::UnknownPending`].
    pub fn deny(&mut self, by: SeatId, id: PendingId) -> Result<(), SessionError> {
        self.require_host(by)?;
        self.withdraw(id)
    }

    /// The host removes another seat (human or bot).
    ///
    /// # Errors
    /// [`SessionError::NotHost`], [`SessionError::CannotKickSelf`] or
    /// [`SessionError::UnknownSeat`].
    pub fn kick(&mut self, by: SeatId, target: SeatId) -> Result<(), SessionError> {
        self.require_host(by)?;
        if by == target {
            return Err(SessionError::CannotKickSelf);
        }
        let index = self.index_of(target)?;
        self.seats.remove(index);
        self.events.push(RoomEvent::Kicked(target));
        self.settle();
        Ok(())
    }

    /// The host adds a bot, which is always ready.
    ///
    /// # Errors
    /// [`SessionError::NotHost`] or [`SessionError::RoomFull`].
    pub fn add_bot(&mut self, by: SeatId, name: PlayerName) -> Result<SeatId, SessionError> {
        self.require_host(by)?;
        self.seat_new(name, Kind::Bot)
    }

    /// Seats a new occupant on the emptiest team (lowest index on ties).
    pub(super) fn seat_new(
        &mut self,
        name: PlayerName,
        kind: Kind,
    ) -> Result<SeatId, SessionError> {
        if self.is_full() {
            return Err(SessionError::RoomFull);
        }
        let capacity = usize::from(self.config.team_capacity());
        let team = (0..self.config.teams())
            .map(TeamId)
            .map(|team| (self.team_count(team), team))
            .filter(|(count, _)| *count < capacity)
            .min()
            .map(|(_, team)| team)
            .ok_or(SessionError::RoomFull)?;
        let id = SeatId(self.next_seat);
        self.next_seat = self.next_seat.wrapping_add(1);
        self.seats.push(Seat {
            id,
            name,
            team,
            ready: matches!(kind, Kind::Bot),
            kind,
        });
        self.events.push(RoomEvent::Seated(id));
        self.settle();
        Ok(id)
    }
}
