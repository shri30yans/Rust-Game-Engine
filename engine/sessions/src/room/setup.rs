//! Lobby setup before a match: teams, ready flags, game settings and admission.
//!
//! Invariant: teams and ready flags change only before the match. A player changes only their
//! own seat's team and ready flag; the host may also move bots between teams.

use crate::error::SessionError;

use super::config::Admission;
use super::event::RoomEvent;
use super::ids::{SeatId, TeamId};
use super::phase::Phase;
use super::state::Room;

impl<S> Room<S> {
    /// Moves a seat to a team: a player moves themselves; the host may also move bots. Only
    /// before the match.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`], [`SessionError::UnknownSeat`], [`SessionError::NotHost`],
    /// [`SessionError::UnknownTeam`] or [`SessionError::TeamFull`].
    pub fn set_team(&mut self, by: SeatId, id: SeatId, team: TeamId) -> Result<(), SessionError> {
        self.require_before_match()?;
        let index = self.index_of(id)?;
        let is_bot = self.seats.get(index).is_some_and(|seat| !seat.is_human());
        if by != id && !(is_bot && self.host == Some(by)) {
            return Err(SessionError::NotHost);
        }
        if team.0 >= self.config.teams() {
            return Err(SessionError::UnknownTeam);
        }
        let seat = self.seats.get(index).ok_or(SessionError::UnknownSeat)?;
        if seat.team == team {
            return Ok(());
        }
        if self.team_count(team) >= usize::from(self.config.team_capacity()) {
            return Err(SessionError::TeamFull);
        }
        if let Some(seat) = self.seats.get_mut(index) {
            seat.team = team;
        }
        self.events.push(RoomEvent::TeamChanged(id, team));
        Ok(())
    }

    /// A human sets their ready flag. Only before the match; unreadying during the countdown
    /// cancels it.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`], [`SessionError::UnknownSeat`] or, for a bot,
    /// [`SessionError::WrongOccupant`].
    pub fn set_ready(&mut self, id: SeatId, ready: bool) -> Result<(), SessionError> {
        self.require_before_match()?;
        let index = self.index_of(id)?;
        let seat = self.seats.get_mut(index).ok_or(SessionError::UnknownSeat)?;
        if !seat.is_human() {
            return Err(SessionError::WrongOccupant);
        }
        if seat.ready != ready {
            seat.ready = ready;
            self.events.push(RoomEvent::ReadyChanged(id, ready));
            self.settle();
        }
        Ok(())
    }

    /// The host changes the game settings, in the lobby only.
    ///
    /// # Errors
    /// [`SessionError::NotHost`] or [`SessionError::WrongPhase`].
    pub fn set_settings(&mut self, by: SeatId, settings: S) -> Result<(), SessionError> {
        self.require_host(by)?;
        if self.phase != Phase::Lobby {
            return Err(SessionError::WrongPhase);
        }
        self.settings = settings;
        self.events.push(RoomEvent::SettingsChanged);
        Ok(())
    }

    /// The host changes who may join, in any phase.
    ///
    /// # Errors
    /// [`SessionError::NotHost`].
    pub fn set_admission(&mut self, by: SeatId, admission: Admission) -> Result<(), SessionError> {
        self.require_host(by)?;
        self.config = self.config.with_admission(admission);
        self.events.push(RoomEvent::SettingsChanged);
        Ok(())
    }
}
