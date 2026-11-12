//! The room's phase: Lobby, Countdown, `InMatch` and Results, in that cycle.
//!
//! Invariant: only the room's own commands change the phase, and each one checks the phase it
//! leaves from (see `cycle.rs` and `setup.rs`).

use engine_core::Tick;
use serde::{Deserialize, Serialize};

/// Where the room is in its cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Players gather, pick teams and ready up.
    Lobby,
    /// The host started; the match begins at `ends_at` unless the start rule breaks first.
    Countdown {
        /// The tick the match begins.
        ends_at: Tick,
    },
    /// The match is running.
    InMatch,
    /// The match ended; players see the results until the server returns to the lobby.
    Results,
}
