//! The game-supplied definition of one sound: file, bus, gain, voice limits and spatial range.
//!
//! Invariant: a definition is checked once when a bank is loaded (see `bank`), never per play.

use std::path::PathBuf;

use crate::attenuation::Attenuation;
use crate::bus::Bus;
use crate::volume::Volume;

/// Data that identifies and configures one decoded sound.
#[derive(Debug, Clone)]
pub struct SoundDef {
    /// Ogg Vorbis or WAV file, supplied by the game.
    pub file: PathBuf,
    /// Output bus.
    pub bus: Bus,
    /// Per-sound gain.
    pub volume: Volume,
    /// Maximum number of simultaneous voices of this sound.
    pub max_voices: usize,
    /// Higher numbers win voice-limit arbitration.
    pub priority: i32,
    /// Spatial attenuation, or `None` for non-spatial playback.
    pub spatial: Option<Attenuation>,
    /// Maximum pitch variation as a fraction around unity.
    pub pitch_jitter: f32,
}
