//! Headless runner: steps an app without a window, a GPU or a render thread.
//!
//! It runs ticks back to back and records the state hash after each one. It
//! never reads the wall clock and never sleeps, so the same app always gives
//! the same hashes.

use crate::app::App;
use crate::error::AppError;
use crate::game::Game;
use crate::startup::{GameEntry, prepare};

/// Builds a game through the same platform, pack and game startup order used by a client process.
pub fn start<E: GameEntry>() -> Result<crate::ClientApp<E::Game, E::Presenter>, AppError> {
    let source = platform::ContentSource::headless();
    let prepared = prepare::<E>(&source)?;
    let mut client = prepared.client;
    client.mount_content(prepared.content)?;
    Ok(client)
}

/// Runs `ticks` ticks and returns the state hash after each one, in order.
///
/// # Errors
///
/// Returns the first error from [`App::step`] or [`App::state_hash`]. Ticks before it have run.
pub fn run_headless<G: Game>(app: &mut App<G>, ticks: u32) -> Result<Vec<u64>, AppError> {
    let capacity = usize::try_from(ticks).map_err(|_| AppError::TickCountRange(ticks))?;
    let mut hashes = Vec::with_capacity(capacity);
    for _ in 0..ticks {
        app.step()?;
        hashes.push(app.state_hash()?);
    }
    Ok(hashes)
}
