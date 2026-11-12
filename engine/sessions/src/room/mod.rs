//! One game session's seats and lifecycle.
//!
//! A [`Room`] is driven by the server: it applies one command at a time, refuses illegal ones
//! with a [`SessionError`], and records what changed as [`RoomEvent`]s for the server to send.
//! The phase cycle is Lobby → Countdown → `InMatch` → Results → Lobby.
//!
//! Invariants, checked after every command by the property tests:
//! - at most `max_seats` seats and at most `team_capacity` per team; seat ids are unique and
//!   increasing, and never reused within a room;
//! - every resume token belongs to at most one seat or queue place;
//! - the host is a human seat whenever a human is seated, and a connected one whenever a
//!   connected human exists; with no humans there is no host;
//! - a room in Countdown always has a host and meets the start rule (enough seats, every
//!   human ready); any command that breaks either drops the room back to Lobby.
//!
//! Dropping in the lobby frees the seat; dropping during a match keeps it (marked
//! disconnected) so the player can resume with their token, and the return to the lobby clears
//! seats nobody came back to.

mod config;
mod cycle;
mod event;
mod ids;
mod membership;
mod pending;
mod phase;
mod seat;
mod setup;
mod state;
mod view;

pub use config::{Admission, MAX_SEATS, MAX_TEAMS, RoomConfig};
pub use event::RoomEvent;
pub use ids::{PendingId, SeatId, TeamId};
pub use pending::JoinOutcome;
pub use phase::Phase;
pub use seat::{Occupant, Seat};
pub use state::Room;
pub use view::{RoomView, SeatView};
