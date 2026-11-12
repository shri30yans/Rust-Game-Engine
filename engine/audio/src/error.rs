//! Typed failures returned by the audio wrapper.
//!
//! Invariant: every failure a caller can cause (bad volume, bad range, unknown id, undecodable
//! file) is a variant here, never a panic.

use std::path::PathBuf;

use thiserror::Error;

/// Failures returned by the audio wrapper.
#[derive(Debug, Error)]
pub enum AudioError {
    /// Kira could not initialise the requested backend.
    #[error("audio backend could not be started")]
    BackendStart,
    /// A sound file could not be decoded.
    #[error("sound file {path} could not be decoded: {source}")]
    SoundFile {
        /// The path that failed to decode.
        path: PathBuf,
        /// The decoder error.
        #[source]
        source: kira::sound::FromFileError,
    },
    /// A requested sound id is not in the loaded bank.
    #[error("unknown sound id {0}")]
    UnknownSound(usize),
    /// Volume must be finite and in [0, 1].
    #[error("volume must be finite and in [0, 1], got {0}")]
    InvalidVolume(f32),
    /// Attenuation distances are invalid.
    #[error("invalid attenuation range {min_distance}..{max_distance}")]
    InvalidAttenuation {
        /// Near full-volume distance.
        min_distance: f32,
        /// Far silence distance.
        max_distance: f32,
    },
    /// The sound definition has invalid pitch jitter or voice count.
    #[error("invalid sound definition for {0}")]
    InvalidSoundDef(PathBuf),
    /// A Kira mixer resource could not be created.
    #[error("audio mixer resource limit was reached")]
    MixerCapacity,
    /// A zero global voice limit cannot admit playback.
    #[error("global voice limit must be at least one")]
    ZeroVoiceLimit,
    /// Embedded sound bytes were missing for a definition.
    #[error("embedded sound data is missing for {0}")]
    EmbeddedSoundMissing(PathBuf),
    /// Embedded sound bytes do not match the number of definitions.
    #[error("sound bank has {expected} definitions and {actual} embedded files")]
    EmbeddedSoundCount {
        /// Number of sound definitions.
        expected: usize,
        /// Number of embedded files supplied.
        actual: usize,
    },
}
