//! Room configuration: seat count, teams, admission, start rules. Built only through
//! [`RoomConfig::new`], so every config a room holds is valid.

use serde::{Deserialize, Serialize};

use crate::error::{ConfigRule, SessionError};

/// Most seats a room can have.
pub const MAX_SEATS: u8 = 64;
/// Most teams a room can have.
pub const MAX_TEAMS: u8 = 8;

/// Who may take a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Admission {
    /// Anyone with the code joins at once.
    Open,
    /// Joiners wait until the host approves them (only while a human host exists; a room with
    /// no humans seats its first human at once so it has someone to approve the rest).
    Approval,
    /// No new players; seated players may still resume.
    Closed,
}

/// A valid room configuration. Decoding runs the same checks as [`RoomConfig::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawConfig")]
pub struct RoomConfig {
    max_seats: u8,
    teams: u8,
    min_to_start: u8,
    admission: Admission,
    countdown_ticks: u32,
}

/// The wire shape of a config before its checks.
#[derive(Deserialize)]
struct RawConfig {
    max_seats: u8,
    teams: u8,
    min_to_start: u8,
    admission: Admission,
    countdown_ticks: u32,
}

impl TryFrom<RawConfig> for RoomConfig {
    type Error = SessionError;

    fn try_from(raw: RawConfig) -> Result<Self, Self::Error> {
        Self::new(
            raw.max_seats,
            raw.teams,
            raw.min_to_start,
            raw.admission,
            raw.countdown_ticks,
        )
    }
}

impl RoomConfig {
    /// Checks and builds a config.
    ///
    /// # Errors
    /// [`SessionError::InvalidConfig`] unless `1 <= max_seats <= MAX_SEATS`,
    /// `1 <= teams <= min(MAX_TEAMS, max_seats)` and `1 <= min_to_start <= max_seats`.
    pub fn new(
        max_seats: u8,
        teams: u8,
        min_to_start: u8,
        admission: Admission,
        countdown_ticks: u32,
    ) -> Result<Self, SessionError> {
        if max_seats == 0 || max_seats > MAX_SEATS {
            return Err(SessionError::InvalidConfig(ConfigRule::Seats));
        }
        if teams == 0 || teams > MAX_TEAMS || teams > max_seats {
            return Err(SessionError::InvalidConfig(ConfigRule::Teams));
        }
        if min_to_start == 0 || min_to_start > max_seats {
            return Err(SessionError::InvalidConfig(ConfigRule::MinToStart));
        }
        Ok(Self {
            max_seats,
            teams,
            min_to_start,
            admission,
            countdown_ticks,
        })
    }

    /// Seats in the room.
    #[must_use]
    pub const fn max_seats(&self) -> u8 {
        self.max_seats
    }

    /// Teams in the room (1 means free-for-all).
    #[must_use]
    pub const fn teams(&self) -> u8 {
        self.teams
    }

    /// Most seats one team may hold: the seats split evenly, rounded up.
    #[must_use]
    pub const fn team_capacity(&self) -> u8 {
        self.max_seats.div_ceil(self.teams)
    }

    /// Filled seats (humans and bots) needed to start.
    #[must_use]
    pub const fn min_to_start(&self) -> u8 {
        self.min_to_start
    }

    /// Who may take a seat.
    #[must_use]
    pub const fn admission(&self) -> Admission {
        self.admission
    }

    /// Ticks between a start and the match beginning.
    #[must_use]
    pub const fn countdown_ticks(&self) -> u32 {
        self.countdown_ticks
    }

    /// The same config with another admission rule.
    #[must_use]
    pub const fn with_admission(self, admission: Admission) -> Self {
        Self { admission, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_outside_the_rules_are_refused() {
        assert!(RoomConfig::new(0, 1, 1, Admission::Open, 0).is_err());
        assert!(RoomConfig::new(65, 1, 1, Admission::Open, 0).is_err());
        assert!(RoomConfig::new(4, 5, 1, Admission::Open, 0).is_err());
        assert!(RoomConfig::new(12, 9, 1, Admission::Open, 0).is_err());
        assert!(RoomConfig::new(4, 2, 5, Admission::Open, 0).is_err());
        assert!(RoomConfig::new(4, 2, 0, Admission::Open, 0).is_err());
        let config = RoomConfig::new(12, 4, 2, Admission::Open, 30);
        assert_eq!(config.map(|c| c.team_capacity()), Ok(3));
        let odd = RoomConfig::new(7, 2, 1, Admission::Open, 0);
        assert_eq!(odd.map(|c| c.team_capacity()), Ok(4));
    }
}
