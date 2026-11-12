//! Sampled animation clip data and validation.
//!
//! Tracks have equal non-empty TRS arrays and refer only to nodes in their owning pack.
use serde::{Deserialize, Serialize};

/// An animation track's sampled linear keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrsChannel {
    /// Node index.
    pub node: u32,
    /// Per-frame translations.
    pub translations: Vec<[f32; 3]>,
    /// Per-frame rotations in x, y, z, w order.
    pub rotations: Vec<[f32; 4]>,
    /// Per-frame scales.
    pub scales: Vec<[f32; 3]>,
}

/// Sampled animation clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Source clip name.
    pub name: String,
    /// Clip duration in seconds.
    pub duration: f32,
    /// Sampling frequency.
    pub fps: f32,
    /// Animated node channels.
    pub channels: Vec<TrsChannel>,
    /// Loop flag only when explicitly present in the source.
    pub looping: Option<bool>,
}

pub(super) fn is_valid(clip: &Clip, node_count: usize) -> bool {
    clip.duration.is_finite()
        && clip.duration >= 0.0
        && clip.fps.is_finite()
        && clip.fps > 0.0
        && clip.channels.iter().all(|channel| {
            usize::try_from(channel.node).is_ok_and(|index| index < node_count)
                && !channel.translations.is_empty()
                && channel.translations.len() == channel.rotations.len()
                && channel.translations.len() == channel.scales.len()
                && channel
                    .translations
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
                && channel
                    .rotations
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
                && channel
                    .scales
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
        })
}
