//! Validated collider construction: shape, local pose, material and layer settings.
//!
//! Each setter validates its value, so a built collider never holds a non-finite or out-of-range
//! parameter.

use super::layers::{CollisionLayers, SurfaceId};
use super::shape::ColliderShape;
use crate::PhysicsError;
use crate::config::{validate_quat, validate_vec3};
use crate::error::{non_negative, positive};
use glam::{Quat, Vec3};
use rapier3d::prelude::{Collider, ColliderBuilder as RapierColliderBuilder};

/// Validated collider construction parameters.
#[derive(Debug, Clone)]
pub struct ColliderBuilder {
    shape: ColliderShape,
    translation: Vec3,
    rotation: Quat,
    density: f32,
    friction: f32,
    restitution: f32,
    sensor: bool,
    layers: CollisionLayers,
    surface: SurfaceId,
}
impl ColliderBuilder {
    /// Starts a builder around a validated shape.
    #[must_use]
    pub fn new(shape: impl Into<ColliderShape>) -> Self {
        Self {
            shape: shape.into(),
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            density: 1.0,
            friction: 0.5,
            restitution: 0.0,
            sensor: false,
            layers: CollisionLayers::ALL,
            surface: SurfaceId::DEFAULT,
        }
    }
    /// Sets local translation.
    pub fn translation(mut self, value: Vec3) -> Result<Self, PhysicsError> {
        validate_vec3(value, "collider translation")?;
        self.translation = value;
        Ok(self)
    }
    /// Sets local rotation.
    pub fn rotation(mut self, value: Quat) -> Result<Self, PhysicsError> {
        validate_quat(value, "collider rotation")?;
        self.rotation = value.normalize();
        Ok(self)
    }
    /// Sets positive density.
    pub fn density(mut self, value: f32) -> Result<Self, PhysicsError> {
        positive(value, "collider density")?;
        self.density = value;
        Ok(self)
    }
    /// Sets non-negative friction.
    pub fn friction(mut self, value: f32) -> Result<Self, PhysicsError> {
        non_negative(value, "collider friction")?;
        self.friction = value;
        Ok(self)
    }
    /// Sets restitution in `[0, 1]`.
    pub fn restitution(mut self, value: f32) -> Result<Self, PhysicsError> {
        if !value.is_finite() {
            return Err(PhysicsError::NonFinite {
                field: "collider restitution",
            });
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(PhysicsError::InvalidValue {
                field: "collider restitution",
                requirement: "between zero and one inclusive",
                value,
            });
        }
        self.restitution = value;
        Ok(self)
    }
    /// Marks the collider as a sensor.
    #[must_use]
    pub const fn sensor(mut self, sensor: bool) -> Self {
        self.sensor = sensor;
        self
    }
    /// Sets physical interaction masks.
    #[must_use]
    pub const fn layers(mut self, layers: CollisionLayers) -> Self {
        self.layers = layers;
        self
    }
    /// Sets the game-owned surface identifier.
    #[must_use]
    pub const fn surface(mut self, surface: SurfaceId) -> Self {
        self.surface = surface;
        self
    }
    /// Returns the geometry.
    #[must_use]
    pub const fn shape(&self) -> &ColliderShape {
        &self.shape
    }
    pub(crate) fn build(self) -> Collider {
        RapierColliderBuilder::new(self.shape.shared())
            .position(crate::convert::to_rapier_pose(
                self.translation,
                self.rotation,
            ))
            .density(self.density)
            .friction(self.friction)
            .restitution(self.restitution)
            .sensor(self.sensor)
            .collision_groups(self.layers.rapier())
            .user_data(u128::from(self.surface.0))
            .build()
    }
}
