//! Explicit state a character carries between fixed ticks, and the result of one fixed movement.
//!
//! `CharacterState::with_motion` rejects a non-finite position or velocity, so every state is
//! finite.

use crate::PhysicsError;
use crate::config::validate_vec3;
use crate::error::finite;
use glam::Vec3;

/// Explicit validated character state carried between fixed ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterState {
    pub(super) position: Vec3,
    pub(super) vertical_velocity: f32,
    pub(super) grounded: bool,
}

impl CharacterState {
    /// Creates an ungrounded stationary state.
    ///
    /// # Errors
    /// Returns an error if the position is non-finite.
    pub fn new(position: Vec3) -> Result<Self, PhysicsError> {
        Self::with_motion(position, 0.0, false)
    }

    /// Creates state with explicit vertical velocity and ground detection state.
    ///
    /// # Errors
    /// Returns an error when the position or velocity is non-finite.
    pub fn with_motion(
        position: Vec3,
        vertical_velocity: f32,
        grounded: bool,
    ) -> Result<Self, PhysicsError> {
        validate_vec3(position, "character position")?;
        finite(vertical_velocity, "character vertical velocity")?;
        Ok(Self {
            position,
            vertical_velocity,
            grounded,
        })
    }

    /// Returns the world-space capsule center.
    #[must_use]
    pub const fn position(self) -> Vec3 {
        self.position
    }

    /// Returns the velocity along world up.
    #[must_use]
    pub const fn vertical_velocity(self) -> f32 {
        self.vertical_velocity
    }

    /// Returns whether the preceding move ended on walkable ground.
    #[must_use]
    pub const fn grounded(self) -> bool {
        self.grounded
    }
}

/// Result of one pure fixed-step character movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterMovement {
    /// State to carry into the next fixed tick.
    pub state: CharacterState,
    /// Collision-constrained world-space translation applied this tick.
    pub translation: Vec3,
}
