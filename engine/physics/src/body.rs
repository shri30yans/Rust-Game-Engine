//! Typed body identifiers, validated builders, and observable body state.

use crate::PhysicsError;
use crate::config::{validate_quat, validate_vec3};
use crate::error::non_negative;
use glam::{Quat, Vec3};
use rapier3d::prelude::{RigidBody, RigidBodyBuilder, RigidBodyHandle};
use serde::{Deserialize, Serialize};

/// Generation-checked identifier for a rigid body owned by one physics world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId {
    index: u32,
    generation: u32,
}

impl BodyId {
    pub(crate) fn from_handle(handle: RigidBodyHandle) -> Self {
        let (index, generation) = handle.into_raw_parts();
        Self { index, generation }
    }

    pub(crate) fn handle(self) -> RigidBodyHandle {
        RigidBodyHandle::from_raw_parts(self.index, self.generation)
    }

    /// Returns the arena slot portion of this identifier.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation portion of this identifier.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// The simulation behavior of a rigid body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyKind {
    /// An immovable body.
    Static,
    /// A force-driven body.
    Dynamic,
    /// A body moved by explicitly setting its next pose.
    Kinematic,
}

/// Validated rigid-body construction parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyBuilder {
    kind: BodyKind,
    translation: Vec3,
    rotation: Quat,
    linear_velocity: Vec3,
    angular_velocity: Vec3,
    linear_damping: f32,
    angular_damping: f32,
    gravity_scale: f32,
    ccd_enabled: bool,
}

impl BodyBuilder {
    /// Starts a static body builder at the origin.
    #[must_use]
    pub const fn static_body() -> Self {
        Self::new(BodyKind::Static)
    }

    /// Starts a dynamic body builder at the origin.
    #[must_use]
    pub const fn dynamic() -> Self {
        Self::new(BodyKind::Dynamic)
    }

    /// Starts a position-based kinematic body builder at the origin.
    #[must_use]
    pub const fn kinematic() -> Self {
        Self::new(BodyKind::Kinematic)
    }

    const fn new(kind: BodyKind) -> Self {
        Self {
            kind,
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            ccd_enabled: false,
        }
    }

    /// Sets the initial world-space translation.
    ///
    /// # Errors
    /// Returns an error when any component is non-finite.
    pub fn translation(mut self, value: Vec3) -> Result<Self, PhysicsError> {
        validate_vec3(value, "body translation")?;
        self.translation = value;
        Ok(self)
    }

    /// Sets and normalizes the initial world-space rotation.
    ///
    /// # Errors
    /// Returns an error for a non-finite or zero quaternion.
    pub fn rotation(mut self, value: Quat) -> Result<Self, PhysicsError> {
        validate_quat(value, "body rotation")?;
        self.rotation = value.normalize();
        Ok(self)
    }

    /// Sets the initial linear velocity.
    ///
    /// # Errors
    /// Returns an error when any component is non-finite.
    pub fn linear_velocity(mut self, value: Vec3) -> Result<Self, PhysicsError> {
        validate_vec3(value, "body linear velocity")?;
        self.linear_velocity = value;
        Ok(self)
    }

    /// Sets the initial angular velocity in radians per second.
    ///
    /// # Errors
    /// Returns an error when any component is non-finite.
    pub fn angular_velocity(mut self, value: Vec3) -> Result<Self, PhysicsError> {
        validate_vec3(value, "body angular velocity")?;
        self.angular_velocity = value;
        Ok(self)
    }

    /// Sets non-negative linear and angular damping coefficients.
    ///
    /// # Errors
    /// Returns an error when either coefficient is negative or non-finite.
    pub fn damping(mut self, linear: f32, angular: f32) -> Result<Self, PhysicsError> {
        non_negative(linear, "linear damping")?;
        non_negative(angular, "angular damping")?;
        self.linear_damping = linear;
        self.angular_damping = angular;
        Ok(self)
    }

    /// Sets a finite gravity multiplier.
    ///
    /// # Errors
    /// Returns an error when the multiplier is non-finite.
    pub fn gravity_scale(mut self, value: f32) -> Result<Self, PhysicsError> {
        if !value.is_finite() {
            return Err(PhysicsError::NonFinite {
                field: "gravity scale",
            });
        }
        self.gravity_scale = value;
        Ok(self)
    }

    /// Enables or disables continuous collision detection.
    #[must_use]
    pub const fn continuous_collision_detection(mut self, enabled: bool) -> Self {
        self.ccd_enabled = enabled;
        self
    }

    /// Returns the body's simulation kind.
    #[must_use]
    pub const fn kind(self) -> BodyKind {
        self.kind
    }

    pub(crate) fn build(self) -> RigidBody {
        let builder = match self.kind {
            BodyKind::Static => RigidBodyBuilder::fixed(),
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
        };
        builder
            .pose(crate::convert::to_rapier_pose(
                self.translation,
                self.rotation,
            ))
            .linvel(crate::convert::to_rapier_vec(self.linear_velocity))
            .angvel(crate::convert::to_rapier_vec(self.angular_velocity))
            .linear_damping(self.linear_damping)
            .angular_damping(self.angular_damping)
            .gravity_scale(self.gravity_scale)
            .ccd_enabled(self.ccd_enabled)
            .build()
    }
}

/// A body pose and velocity snapshot at one simulation tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodyState {
    /// World-space center translation.
    pub translation: Vec3,
    /// World-space orientation.
    pub rotation: Quat,
    /// Linear velocity in world units per second.
    pub linear_velocity: Vec3,
    /// Angular velocity in radians per second.
    pub angular_velocity: Vec3,
    /// Whether the body is sleeping.
    pub sleeping: bool,
}
