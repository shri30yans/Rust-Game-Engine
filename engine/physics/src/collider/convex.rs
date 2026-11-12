//! Validated convex shapes for moving bodies, casts and collider geometry.
//!
//! A `ConvexShape` is built only through a validating constructor, so every instance has
//! non-degenerate geometry. Rapier shape handles are shared, so cloning does not duplicate
//! cooked vertex data.

use std::sync::Arc;

use crate::PhysicsError;
use crate::config::validate_vec3;
use crate::error::{non_negative, positive};
use glam::Vec3;
use rapier3d::prelude::{ColliderBuilder as RapierColliderBuilder, SharedShape};

/// A validated convex shape suitable for moving bodies and casts.
#[derive(Debug, Clone)]
pub struct ConvexShape(Arc<SharedShape>);
impl ConvexShape {
    pub(crate) fn valid_capsule(half_height: f32, radius: f32) -> Self {
        Self(Arc::new(SharedShape::capsule_y(half_height, radius)))
    }
    /// Creates a sphere.
    pub fn ball(radius: f32) -> Result<Self, PhysicsError> {
        positive(radius, "ball radius")?;
        Ok(Self(Arc::new(SharedShape::ball(radius))))
    }
    /// Creates a cuboid.
    pub fn cuboid(half_extents: Vec3) -> Result<Self, PhysicsError> {
        validate_vec3(half_extents, "cuboid half extents")?;
        positive(half_extents.x, "cuboid half extent x")?;
        positive(half_extents.y, "cuboid half extent y")?;
        positive(half_extents.z, "cuboid half extent z")?;
        Ok(Self(Arc::new(SharedShape::cuboid(
            half_extents.x,
            half_extents.y,
            half_extents.z,
        ))))
    }
    /// Creates a vertical capsule.
    pub fn capsule(half_height: f32, radius: f32) -> Result<Self, PhysicsError> {
        non_negative(half_height, "capsule half height")?;
        positive(radius, "capsule radius")?;
        Ok(Self(Arc::new(SharedShape::capsule_y(half_height, radius))))
    }
    /// Builds a convex hull and rejects non-finite, too-small, coplanar or otherwise degenerate points.
    pub fn hull(points: &[[f32; 3]]) -> Result<Self, PhysicsError> {
        if points.iter().flatten().any(|point| !point.is_finite()) {
            return Err(PhysicsError::NonFinite {
                field: "convex hull points",
            });
        }
        let vertices = points
            .iter()
            .map(|point| crate::convert::to_rapier_vec(Vec3::from_array(*point)))
            .collect::<Vec<_>>();
        let hull = SharedShape::convex_hull(&vertices).ok_or(PhysicsError::DegenerateHull)?;
        let volume = RapierColliderBuilder::new(hull.clone())
            .density(1.0)
            .build()
            .mass();
        if !volume.is_finite() || volume <= f32::EPSILON {
            return Err(PhysicsError::DegenerateHull);
        }
        Ok(Self(Arc::new(hull)))
    }
    pub(crate) fn shared(&self) -> SharedShape {
        (*self.0).clone()
    }
    /// Returns the convex shape's volume in cubic metres.
    #[must_use]
    pub fn volume(&self) -> f32 {
        RapierColliderBuilder::new(self.shared())
            .density(1.0)
            .build()
            .mass()
    }
}
