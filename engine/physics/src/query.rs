//! Validated ray and shape-cast requests with typed hit results.

use crate::config::{validate_quat, validate_vec3};
use crate::error::positive;
use crate::{BodyId, ColliderOwner, ConvexShape, LayerMask, PhysicsError, SurfaceId};
use glam::{Quat, Vec3};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter as RapierQueryFilter};

/// Filter shared by every physics query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryFilter {
    /// Collider membership bits accepted by the query.
    pub layers: LayerMask,
    /// Optional body whose every collider is omitted.
    pub exclude_body: Option<BodyId>,
    /// Whether sensors participate.
    pub include_sensors: bool,
}
impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            layers: LayerMask::ALL,
            exclude_body: None,
            include_sensors: false,
        }
    }
}
impl QueryFilter {
    /// Excludes every collider attached to one body while preserving this filter's layers.
    #[must_use]
    pub const fn excluding_body(mut self, body: BodyId) -> Self {
        self.exclude_body = Some(body);
        self
    }

    /// Whether this query accepts a collider's sensor and membership properties.
    pub(crate) fn accepts(&self, collider: &Collider) -> bool {
        (self.include_sensors || !collider.is_sensor())
            && (self.layers.bits() & collider.collision_groups().memberships.bits()) != 0
    }

    /// Builds Rapier's query filter with this filter's body exclusion and acceptance predicate.
    pub(crate) fn rapier_filter<'a>(
        &self,
        predicate: &'a dyn Fn(ColliderHandle, &Collider) -> bool,
    ) -> RapierQueryFilter<'a> {
        RapierQueryFilter {
            exclude_rigid_body: self.exclude_body.map(BodyId::handle),
            predicate: Some(predicate),
            ..RapierQueryFilter::default()
        }
    }
}

/// A finite ray segment used for scene queries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayCast {
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    solid: bool,
}

impl RayCast {
    /// Creates a ray whose direction is normalized during construction.
    ///
    /// # Errors
    /// Returns an error for non-finite input, a zero direction, or a non-positive distance.
    pub fn new(
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        solid: bool,
    ) -> Result<Self, PhysicsError> {
        validate_vec3(origin, "ray origin")?;
        validate_vec3(direction, "ray direction")?;
        positive(max_distance, "ray max distance")?;
        let length_squared = direction.length_squared();
        if !length_squared.is_finite() {
            return Err(PhysicsError::NonFinite {
                field: "ray direction length",
            });
        }
        if length_squared <= f32::EPSILON {
            return Err(PhysicsError::InvalidValue {
                field: "ray direction length",
                requirement: "greater than zero",
                value: direction.length(),
            });
        }
        Ok(Self {
            origin,
            direction: direction.normalize(),
            max_distance,
            solid,
        })
    }

    pub(crate) const fn origin(self) -> Vec3 {
        self.origin
    }

    pub(crate) const fn direction(self) -> Vec3 {
        self.direction
    }

    pub(crate) const fn max_distance(self) -> f32 {
        self.max_distance
    }

    pub(crate) const fn solid(self) -> bool {
        self.solid
    }
}

/// Closest ray intersection in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayCastHit {
    /// Typed collider and optional body identifiers.
    pub owner: ColliderOwner,
    /// Distance from the ray origin because requests normalize direction.
    pub distance: f32,
    /// World-space point on the hit collider.
    pub point: Vec3,
    /// Outward world-space surface normal.
    pub normal: Vec3,
    /// Physical surface identifier assigned to the collider.
    pub surface: SurfaceId,
}

/// A translational cast of a validated convex shape.
#[derive(Debug, Clone)]
pub struct ShapeCast {
    shape: ConvexShape,
    translation: Vec3,
    rotation: Quat,
    direction: Vec3,
    max_distance: f32,
    stop_at_penetration: bool,
}

impl ShapeCast {
    /// Creates a shape cast whose direction is normalized during construction.
    ///
    /// # Errors
    /// Returns an error for invalid pose data, zero direction, or non-positive distance.
    pub fn new(
        shape: ConvexShape,
        translation: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
    ) -> Result<Self, PhysicsError> {
        validate_vec3(translation, "shape cast translation")?;
        validate_quat(rotation, "shape cast rotation")?;
        validate_vec3(direction, "shape cast direction")?;
        positive(max_distance, "shape cast max distance")?;
        let length_squared = direction.length_squared();
        if !length_squared.is_finite() {
            return Err(PhysicsError::NonFinite {
                field: "shape cast direction length",
            });
        }
        if length_squared <= f32::EPSILON {
            return Err(PhysicsError::InvalidValue {
                field: "shape cast direction length",
                requirement: "greater than zero",
                value: direction.length(),
            });
        }
        Ok(Self {
            shape,
            translation,
            rotation: rotation.normalize(),
            direction: direction.normalize(),
            max_distance,
            stop_at_penetration: true,
        })
    }

    /// Selects whether an initially penetrating cast stops immediately.
    #[must_use]
    pub const fn stop_at_penetration(mut self, enabled: bool) -> Self {
        self.stop_at_penetration = enabled;
        self
    }

    pub(crate) fn shape(&self) -> &ConvexShape {
        &self.shape
    }

    pub(crate) fn translation(&self) -> Vec3 {
        self.translation
    }

    pub(crate) fn rotation(&self) -> Quat {
        self.rotation
    }

    pub(crate) fn velocity(&self) -> Vec3 {
        self.direction * self.max_distance
    }

    pub(crate) fn stop_at_penetration_value(&self) -> bool {
        self.stop_at_penetration
    }
}

/// Closest shape-cast impact in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeCastHit {
    /// Typed collider and optional body identifiers.
    pub owner: ColliderOwner,
    /// Distance traveled before impact.
    pub distance: f32,
    /// Physical surface identifier assigned to the collider.
    pub surface: SurfaceId,
}
