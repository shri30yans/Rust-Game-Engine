//! Sessions: the lobby layer between "a player opened the game" and "a match is running".
//!
//! A [`Room`] owns the seats of
//! one game session (humans and bots, teams, ready flags, the host, admission and the
//! lobby → countdown → match → results cycle), and a [`SessionDirectory`] lists the rooms a
//! server hosts so players can find one by [`RoomCode`] or by browsing.
//!
//! Invariants:
//! - Everything here is pure and tick-driven: no I/O, no wall clock, no global state. The
//!   server feeds commands and the current [`Tick`](engine_core::Tick) in, and sends
//!   [`RoomView`]s and [`RoomEvent`]s out over the network.
//! - The only entropy source is [`ResumeToken::generate`] (OS randomness, native only); room
//!   codes come from a seeded [`SimRng`](engine_core::SimRng) the server seeds once.
//! - [`LobbyHost`] speaks the lobby protocol for one room over any transport: connection ids
//!   are a type parameter, so this crate does not depend on the network layer.
//! - Game rules stay in the game: a room carries game-defined settings `S` and a directory
//!   carries a game-defined listing `L`, both opaque here.
#![forbid(unsafe_code)]

mod codes;
mod directory;
mod error;
mod lobby;
mod name;
mod room;

pub use codes::{ROOM_CODE_ALPHABET, ROOM_CODE_LEN, ResumeToken, RoomCode};
pub use directory::{DirectoryEntry, SessionDirectory};
pub use error::{ConfigRule, SessionError};
#[cfg(not(target_arch = "wasm32"))]
pub use lobby::OsTokens;
pub use lobby::{LobbyHost, LobbyReply, LobbyRequest, Membership, TokenSource};
pub use name::{MAX_NAME_CHARS, PlayerName};
pub use room::{
    Admission, JoinOutcome, MAX_SEATS, MAX_TEAMS, Occupant, PendingId, Phase, Room, RoomConfig,
    RoomEvent, RoomView, Seat, SeatId, SeatView, TeamId,
};
