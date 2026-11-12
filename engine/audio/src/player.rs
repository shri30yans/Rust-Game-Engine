//! The backend-independent playback surface used by games and headless tests.
//!
//! Invariant: games talk to `AudioPlayer`, never to Kira, so the device and the mock backend are
//! interchangeable.

use glam::Vec3;

use crate::bank::{SoundBank, SoundId};
use crate::error::AudioError;

/// How a one-shot is emitted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Emit {
    /// Emit at a world-space position.
    At(Vec3),
    /// Emit through the non-spatial UI path.
    Ui,
}

/// The backend-independent playback surface used by games and headless tests.
pub trait AudioPlayer {
    /// Replaces the decoded sound set.
    fn set_bank(&mut self, bank: SoundBank) -> Result<(), AudioError>;
    /// Plays one sound id.
    fn play(&mut self, id: SoundId, emit: Emit) -> Result<(), AudioError>;
    /// Number of active voices known to the engine.
    fn active_voices(&self) -> usize;
}
