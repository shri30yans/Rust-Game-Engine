//! Validated fixed-step world configuration.

use crate::PhysicsError;
use crate::error::{finite, positive};
use glam::Vec3;

/// Immutable configuration used by every [`crate::PhysicsWorld`] step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsConfig {
    dt: f32,
    gravity: Vec3,
}

impl PhysicsConfig {
    /// Creates a fixed-step configuration.
    ///
    /// # Errors
    /// Returns an error when `dt` is not finite and positive or gravity is not finite.
    pub fn new(dt: f32, gravity: Vec3) -> Result<Self, PhysicsError> {
        positive(dt, "dt")?;
        if !gravity.is_finite() {
            return Err(PhysicsError::NonFinite { field: "gravity" });
        }
        Ok(Self { dt, gravity })
    }

    /// Returns the fixed simulation interval in seconds.
    #[must_use]
    pub const fn dt(self) -> f32 {
        self.dt
    }

    /// Returns the constant acceleration applied to gravity-enabled dynamic bodies.
    #[must_use]
    pub const fn gravity(self) -> Vec3 {
        self.gravity
    }
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            dt: 1.0 / 60.0,
            gravity: Vec3::new(0.0, -9.81, 0.0),
        }
    }
}

pub(crate) fn validate_vec3(value: Vec3, field: &'static str) -> Result<(), PhysicsError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(PhysicsError::NonFinite { field })
    }
}

pub(crate) fn validate_quat(value: glam::Quat, field: &'static str) -> Result<(), PhysicsError> {
    if !value.is_finite() {
        return Err(PhysicsError::NonFinite { field });
    }
    finite(value.length_squared(), field)?;
    if value.length_squared() <= f32::EPSILON {
        return Err(PhysicsError::ZeroQuaternion { field });
    }
    Ok(())
}
