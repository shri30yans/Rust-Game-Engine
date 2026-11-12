//! The one error type of the crate. Every refused command names why, so the server can tell
//! the player and tests can assert the exact rule that fired. Refusals travel to clients, so
//! the type is serialisable.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why a session command was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum SessionError {
    /// A room config broke one of its rules.
    #[error("invalid room config: {0}")]
    InvalidConfig(ConfigRule),
    /// A player name was empty, too long, or held control characters.
    #[error("player names are 1 to 16 printable characters")]
    InvalidName,
    /// The text is not a room code.
    #[error("room codes are 6 characters from the room code alphabet")]
    InvalidCode,
    /// Every seat is taken.
    #[error("the room is full")]
    RoomFull,
    /// The approval queue is full.
    #[error("too many players are waiting for approval")]
    QueueFull,
    /// The room accepts no new players (resume still works).
    #[error("the room is closed to new players")]
    Closed,
    /// This token already owns a seat or a queue place; resume instead.
    #[error("this player is already in the room")]
    AlreadyInRoom,
    /// No seat has this token.
    #[error("no seat belongs to this player")]
    UnknownToken,
    /// No seat has this id.
    #[error("no such seat")]
    UnknownSeat,
    /// No queued player has this id.
    #[error("no such waiting player")]
    UnknownPending,
    /// Only the host may do this.
    #[error("only the host may do this")]
    NotHost,
    /// The host cannot kick their own seat.
    #[error("the host cannot kick themselves")]
    CannotKickSelf,
    /// The command needs a human seat and this one is a bot (or the reverse).
    #[error("this seat is the wrong kind for the command")]
    WrongOccupant,
    /// The team id is not one of the room's teams.
    #[error("no such team")]
    UnknownTeam,
    /// The team already has its share of the seats.
    #[error("that team is full")]
    TeamFull,
    /// The command is not allowed in the room's current phase.
    #[error("not allowed in this phase")]
    WrongPhase,
    /// A human is not ready yet.
    #[error("not every player is ready")]
    NotAllReady,
    /// Fewer seats are filled than the room needs to start.
    #[error("not enough players to start")]
    NotEnoughPlayers,
    /// The directory could not find a free room code.
    #[error("no free room code was found")]
    CodesExhausted,
    /// No room in the directory has this code.
    #[error("no room has this code")]
    UnknownRoom,
    /// The operating system could not supply randomness for a token.
    #[error("the system random source failed")]
    Entropy,
}

/// The room config rule that was broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum ConfigRule {
    /// Seats must be 1 to [`MAX_SEATS`](crate::MAX_SEATS).
    #[error("seats must be 1 to 64")]
    Seats,
    /// Teams must be 1 to [`MAX_TEAMS`](crate::MAX_TEAMS) and at most the seats.
    #[error("teams must be 1 to 8 and at most the seats")]
    Teams,
    /// Players needed to start must be 1 to the seats.
    #[error("players to start must be 1 to the seats")]
    MinToStart,
}
