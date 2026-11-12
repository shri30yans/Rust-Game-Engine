//! Sphere blasts with deterministic distance falloff.
//!
//! Damage depends only on the distance from the centre, and rounds to the
//! nearest integer with halves going up. Rounding uses an integer search rather
//! than a float-to-integer cast, so it cannot truncate.

use std::num::NonZeroU16;

use glam::Vec3;

use crate::error::{DestructionError, require_finite_point, require_positive};

/// How damage drops off with distance from the blast centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Falloff {
    /// Damage fraction falls linearly from 1 at the centre to 0 at the radius.
    Linear,
    /// Damage fraction falls with the square of the linear term.
    Quadratic,
}

/// A sphere that damages elements within `radius` of `center`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    center: Vec3,
    radius: f32,
    peak_damage: NonZeroU16,
    falloff: Falloff,
}

impl Blast {
    /// Creates a blast. The centre must be finite and the radius positive.
    ///
    /// # Errors
    /// Returns an error for a non-finite centre or a radius that is not finite and positive.
    pub fn new(
        center: Vec3,
        radius: f32,
        peak_damage: NonZeroU16,
        falloff: Falloff,
    ) -> Result<Self, DestructionError> {
        Ok(Self {
            center: require_finite_point("center", center)?,
            radius: require_positive("radius", radius)?,
            peak_damage,
            falloff,
        })
    }

    /// Damage an element at `position` takes from this blast. Zero outside the radius.
    #[must_use]
    pub(crate) fn damage_at(&self, position: Vec3) -> u16 {
        let distance = (position - self.center).length();
        if distance > self.radius {
            return 0;
        }
        let linear = (1.0 - distance / self.radius).clamp(0.0, 1.0);
        let fraction = match self.falloff {
            Falloff::Linear => linear,
            Falloff::Quadratic => linear * linear,
        };
        round_to_u16(
            fraction * f32::from(self.peak_damage.get()),
            self.peak_damage.get(),
        )
    }
}

/// Rounds `value` (already within `0..=max`) to the nearest integer, halves up,
/// by binary search over the integers. Never casts a float.
fn round_to_u16(value: f32, max: u16) -> u16 {
    let mut low = 0u16;
    let mut high = max;
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if f32::from(mid) - 0.5 <= value {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn peak(value: u16) -> NonZeroU16 {
        NonZeroU16::new(value).unwrap()
    }

    #[test]
    fn linear_falloff_hand_computed() {
        let blast = Blast::new(Vec3::ZERO, 4.0, peak(100), Falloff::Linear).unwrap();
        assert_eq!(blast.damage_at(Vec3::ZERO), 100);
        assert_eq!(blast.damage_at(Vec3::new(1.0, 0.0, 0.0)), 75);
        assert_eq!(blast.damage_at(Vec3::new(2.0, 0.0, 0.0)), 50);
        assert_eq!(blast.damage_at(Vec3::new(4.0, 0.0, 0.0)), 0);
        assert_eq!(blast.damage_at(Vec3::new(5.0, 0.0, 0.0)), 0);
    }

    #[test]
    fn quadratic_falloff_hand_computed() {
        let blast = Blast::new(Vec3::ZERO, 4.0, peak(100), Falloff::Quadratic).unwrap();
        assert_eq!(blast.damage_at(Vec3::new(2.0, 0.0, 0.0)), 25);
    }

    #[test]
    fn rounds_halves_up() {
        assert_eq!(round_to_u16(2.4, 10), 2);
        assert_eq!(round_to_u16(2.5, 10), 3);
        assert_eq!(round_to_u16(0.0, 10), 0);
        assert_eq!(round_to_u16(10.0, 10), 10);
    }

    #[test]
    fn rejects_bad_geometry() {
        assert!(Blast::new(Vec3::new(f32::NAN, 0.0, 0.0), 1.0, peak(1), Falloff::Linear).is_err());
        assert!(Blast::new(Vec3::ZERO, 0.0, peak(1), Falloff::Linear).is_err());
    }
}
