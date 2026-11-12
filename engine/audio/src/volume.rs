//! Linear gain, validated once at construction.
//!
//! Invariant: a `Volume` is always finite and within [0, 1]. Conversion to decibels happens only
//! inside the crate, when a Kira track or sound is given its gain.

use crate::error::AudioError;

const SILENCE_DB: f32 = -60.0;

/// A finite linear volume between silence and unity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume(pub(crate) f32);

impl Volume {
    /// Creates a validated volume.
    pub fn new(linear: f32) -> Result<Self, AudioError> {
        if linear.is_finite() && (0.0..=1.0).contains(&linear) {
            Ok(Self(linear))
        } else {
            Err(AudioError::InvalidVolume(linear))
        }
    }

    /// The linear gain.
    #[must_use]
    pub const fn linear(self) -> f32 {
        self.0
    }

    pub(crate) fn decibels(self) -> f32 {
        if self.0 == 0.0 {
            SILENCE_DB
        } else {
            20.0 * self.0.log10()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn volume_rejects_non_finite_and_out_of_range_values() {
        assert!(Volume::new(f32::NAN).is_err());
        assert!(Volume::new(-0.01).is_err());
        assert!(Volume::new(1.01).is_err());
        assert_eq!(Volume::new(0.5).unwrap().linear(), 0.5);
    }
}
