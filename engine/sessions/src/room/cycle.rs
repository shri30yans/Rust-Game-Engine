//! The match cycle: countdown, match start, match end and return to the lobby.
//!
//! Invariant: the phase moves only Lobby → Countdown → `InMatch` → Results → Lobby. A countdown
//! that loses its start rule falls back to the lobby in `settle`.

use engine_core::Tick;

use crate::error::SessionError;

use super::event::RoomEvent;
use super::ids::SeatId;
use super::phase::Phase;
use super::seat::Kind;
use super::state::Room;

impl<S> Room<S> {
    /// The host starts the countdown.
    ///
    /// # Errors
    /// [`SessionError::NotHost`], [`SessionError::WrongPhase`],
    /// [`SessionError::NotEnoughPlayers`] or [`SessionError::NotAllReady`].
    pub fn start(&mut self, by: SeatId, now: Tick) -> Result<(), SessionError> {
        self.require_host(by)?;
        if self.phase != Phase::Lobby {
            return Err(SessionError::WrongPhase);
        }
        if self.seats.len() < usize::from(self.config.min_to_start()) {
            return Err(SessionError::NotEnoughPlayers);
        }
        if !self.can_start() {
            return Err(SessionError::NotAllReady);
        }
        let ends_at = now.advance(self.config.countdown_ticks());
        self.phase = Phase::Countdown { ends_at };
        self.events.push(RoomEvent::CountdownStarted(ends_at));
        Ok(())
    }

    /// The host stops the countdown.
    ///
    /// # Errors
    /// [`SessionError::NotHost`] or [`SessionError::WrongPhase`].
    pub fn cancel_start(&mut self, by: SeatId) -> Result<(), SessionError> {
        self.require_host(by)?;
        if !matches!(self.phase, Phase::Countdown { .. }) {
            return Err(SessionError::WrongPhase);
        }
        self.phase = Phase::Lobby;
        self.events.push(RoomEvent::CountdownCancelled);
        Ok(())
    }

    /// Advances time: a finished countdown begins the match.
    pub fn tick(&mut self, now: Tick) {
        if let Phase::Countdown { ends_at } = self.phase
            && !now.is_before(ends_at)
        {
            self.phase = Phase::InMatch;
            self.events.push(RoomEvent::MatchStarted);
        }
    }

    /// The game reports the match is over.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`] unless a match is running.
    pub fn end_match(&mut self) -> Result<(), SessionError> {
        if self.phase != Phase::InMatch {
            return Err(SessionError::WrongPhase);
        }
        self.phase = Phase::Results;
        self.events.push(RoomEvent::MatchEnded);
        Ok(())
    }

    /// Back to the lobby after the results: seats nobody resumed are freed and humans must
    /// ready again.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`] unless showing results.
    pub fn return_to_lobby(&mut self) -> Result<(), SessionError> {
        if self.phase != Phase::Results {
            return Err(SessionError::WrongPhase);
        }
        let events = &mut self.events;
        self.seats.retain(|seat| {
            let keep = !matches!(
                seat.kind,
                Kind::Human {
                    connected: false,
                    ..
                }
            );
            if !keep {
                events.push(RoomEvent::Left(seat.id));
            }
            keep
        });
        for seat in &mut self.seats {
            if seat.is_human() {
                seat.ready = false;
            }
        }
        self.phase = Phase::Lobby;
        self.events.push(RoomEvent::ReturnedToLobby);
        self.settle();
        Ok(())
    }
}
