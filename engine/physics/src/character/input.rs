//! Validated per-tick character input.
//!
//! Ground velocity is always horizontal: `CharacterInput::new` removes its vertical component.

use crate::PhysicsError;
use crate::config::validate_vec3;
use glam::Vec3;

/// Validated input consumed during one fixed character movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterInput {
    pub(super) ground_velocity: Vec3,
    pub(super) jump: bool,
}

impl CharacterInput {
    /// Creates validated character input and removes any vertical velocity component.
    ///
    /// # Errors
    /// Returns an error if velocity is non-finite.
    pub fn new(ground_velocity: Vec3, jump: bool) -> Result<Self, PhysicsError> {
        validate_vec3(ground_velocity, "character ground velocity")?;
        Ok(Self {
            ground_velocity: Vec3::new(ground_velocity.x, 0.0, ground_velocity.z),
            jump,
        })
    }

    /// Returns requested velocity along the horizontal XZ plane.
    #[must_use]
    pub const fn ground_velocity(self) -> Vec3 {
        self.ground_velocity
    }

    /// Returns whether jump was pressed during this tick.
    #[must_use]
    pub const fn jump(self) -> bool {
        self.jump
    }
}
