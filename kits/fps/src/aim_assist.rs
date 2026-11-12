//! Aim slowdown for touch look. This module only compares a view ray with caller-supplied target
//! spheres; it has no world access and allocates nothing.

use glam::Vec3;
use thiserror::Error;

/// Invalid aim-assist tuning rejected when it is built.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum AimAssistError {
    /// The outer slowdown cone must be finite and positive.
    #[error("cone_deg must be finite and positive, got {0}")]
    InvalidCone(f32),
    /// The slowdown scale must lie in `(0, 1]`.
    #[error("slowdown must be finite and in (0, 1], got {0}")]
    InvalidSlowdown(f32),
    /// The maximum target range must be finite and positive.
    #[error("max_range_m must be finite and positive, got {0}")]
    InvalidRange(f32),
}

/// A target sphere for aim slowdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimTarget {
    /// Sphere centre in world space.
    pub centre: Vec3,
    /// Sphere radius in metres.
    pub radius: f32,
}

/// Tuning for aim slowdown. `slowdown` is the scale on the target and `cone_deg` is the outer
/// angular edge of the linear recovery ramp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimAssistParams {
    /// Outer ramp angle, in degrees.
    cone_deg: f32,
    /// Scale on a target, in `(0, 1]`.
    slowdown: f32,
    /// Maximum target distance in metres.
    max_range_m: f32,
}

impl AimAssistParams {
    /// Builds validated aim-assist tuning.
    ///
    /// # Errors
    ///
    /// Returns the specific invalid field when any value is outside its accepted range.
    pub fn new(cone_deg: f32, slowdown: f32, max_range_m: f32) -> Result<Self, AimAssistError> {
        if !cone_deg.is_finite() || cone_deg <= 0.0 {
            return Err(AimAssistError::InvalidCone(cone_deg));
        }
        if !slowdown.is_finite() || !(0.0..=1.0).contains(&slowdown) || slowdown == 0.0 {
            return Err(AimAssistError::InvalidSlowdown(slowdown));
        }
        if !max_range_m.is_finite() || max_range_m <= 0.0 {
            return Err(AimAssistError::InvalidRange(max_range_m));
        }
        Ok(Self {
            cone_deg,
            slowdown,
            max_range_m,
        })
    }

    /// Outer recovery ramp angle, in degrees.
    #[must_use]
    pub const fn cone_deg(self) -> f32 {
        self.cone_deg
    }

    /// Look scale at the centre of a target sphere.
    #[must_use]
    pub const fn slowdown(self) -> f32 {
        self.slowdown
    }

    /// Maximum distance at which aim slowdown applies, in metres.
    #[must_use]
    pub const fn max_range_m(self) -> f32 {
        self.max_range_m
    }
}

/// Computes the look scale for the nearest-angle target sphere.
pub struct AimAssist;

impl AimAssist {
    /// Returns the look multiplier for the supplied targets.
    #[must_use]
    pub fn look_scale(
        eye: Vec3,
        forward: Vec3,
        targets: &[AimTarget],
        params: AimAssistParams,
    ) -> f32 {
        if targets.is_empty() || !eye.is_finite() || !forward.is_finite() {
            return 1.0;
        }
        let direction = forward.normalize_or_zero();
        if direction == Vec3::ZERO {
            return 1.0;
        }
        let cone = params.cone_deg.to_radians();
        let slowdown = params.slowdown;
        let mut best = 1.0_f32;
        let mut best_angle = f32::INFINITY;
        for target in targets {
            if !target.centre.is_finite() || !target.radius.is_finite() || target.radius < 0.0 {
                continue;
            }
            let offset = target.centre - eye;
            let distance = offset.length();
            if !distance.is_finite() || distance <= 0.0 || distance > params.max_range_m {
                continue;
            }
            let angle = direction.angle_between(offset / distance);
            if !angle.is_finite() {
                continue;
            }
            let angular_radius = (target.radius.max(0.0) / distance).clamp(0.0, 1.0).asin();
            let outside_sphere = (angle - angular_radius).max(0.0);
            if outside_sphere >= best_angle {
                continue;
            }
            best_angle = outside_sphere;
            best = if outside_sphere <= 0.0 {
                slowdown
            } else {
                (slowdown + (1.0 - slowdown) * outside_sphere / cone).min(1.0)
            };
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const PARAMS: AimAssistParams = AimAssistParams {
        cone_deg: 4.0,
        slowdown: 0.65,
        max_range_m: 40.0,
    };

    fn target(z: f32) -> AimTarget {
        AimTarget {
            centre: Vec3::new(0.0, 0.0, z),
            radius: 0.5,
        }
    }

    #[test]
    fn no_targets_and_targets_beyond_range_do_not_slow_look() {
        assert_eq!(
            AimAssist::look_scale(Vec3::ZERO, Vec3::NEG_Z, &[], PARAMS),
            1.0
        );
        assert_eq!(
            AimAssist::look_scale(Vec3::ZERO, Vec3::NEG_Z, &[target(-41.0)], PARAMS),
            1.0
        );
    }

    #[test]
    fn aim_through_target_sphere_uses_full_slowdown() {
        assert_eq!(
            AimAssist::look_scale(Vec3::ZERO, Vec3::NEG_Z, &[target(-10.0)], PARAMS),
            0.65
        );
    }

    #[test]
    fn ramp_is_continuous_and_monotonic() {
        let scales: Vec<f32> = (0_u8..=8)
            .map(|step| {
                let angle = f32::from(step) * 0.5_f32.to_radians();
                AimAssist::look_scale(
                    Vec3::ZERO,
                    Vec3::new(angle.sin(), 0.0, -angle.cos()),
                    &[target(-10.0)],
                    PARAMS,
                )
            })
            .collect();
        assert!(
            scales
                .windows(2)
                .all(|pair| { pair.first().zip(pair.get(1)).is_some_and(|(a, b)| a <= b) })
        );
        let first_pair = scales.first().zip(scales.get(1));
        assert!(first_pair.is_some_and(|(a, b)| (*b - *a).abs() < 0.2));
    }

    proptest! {
        #[test]
        fn scale_stays_between_slowdown_and_one(angle in -1.0f32..1.0) {
            let forward = Vec3::new(angle.sin(), 0.0, -angle.cos());
            let scale = AimAssist::look_scale(Vec3::ZERO, forward, &[target(-10.0)], PARAMS);
            prop_assert!((PARAMS.slowdown..=1.0).contains(&scale));
        }
    }

    #[test]
    fn invalid_parameters_return_typed_errors() {
        assert!(matches!(
            AimAssistParams::new(0.0, 0.65, 40.0),
            Err(AimAssistError::InvalidCone(_))
        ));
        assert!(matches!(
            AimAssistParams::new(f32::INFINITY, 0.65, 40.0),
            Err(AimAssistError::InvalidCone(_))
        ));
        assert!(matches!(
            AimAssistParams::new(4.0, 0.0, 40.0),
            Err(AimAssistError::InvalidSlowdown(_))
        ));
        assert!(matches!(
            AimAssistParams::new(4.0, f32::NAN, 40.0),
            Err(AimAssistError::InvalidSlowdown(_))
        ));
        assert!(matches!(
            AimAssistParams::new(4.0, 0.65, 0.0),
            Err(AimAssistError::InvalidRange(_))
        ));
        assert!(matches!(
            AimAssistParams::new(4.0, 0.65, f32::INFINITY),
            Err(AimAssistError::InvalidRange(_))
        ));
    }
}
