//! Spatial distance range for linear-in-decibels attenuation.
//!
//! Invariant: distances are finite, the near distance is non-negative and the far distance is
//! strictly greater, so a constructed `Attenuation` is always a valid range.

use crate::error::AudioError;

/// Distance range for linear-in-decibels attenuation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attenuation {
    /// Distance at which full volume begins, in metres.
    min_distance: f32,
    /// Distance at which the sound reaches silence, in metres.
    max_distance: f32,
}

impl Attenuation {
    /// Validates a spatial attenuation range.
    pub fn new(min_distance: f32, max_distance: f32) -> Result<Self, AudioError> {
        if min_distance.is_finite()
            && max_distance.is_finite()
            && min_distance >= 0.0
            && max_distance > min_distance
        {
            Ok(Self {
                min_distance,
                max_distance,
            })
        } else {
            Err(AudioError::InvalidAttenuation {
                min_distance,
                max_distance,
            })
        }
    }

    /// Near full-volume distance in metres.
    #[must_use]
    pub const fn min_distance(self) -> f32 {
        self.min_distance
    }

    /// Far silence distance in metres.
    #[must_use]
    pub const fn max_distance(self) -> f32 {
        self.max_distance
    }
}
