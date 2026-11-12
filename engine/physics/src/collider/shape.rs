//! The collider geometry sum type, and its conversion from cooked asset collision records.
//!
//! Only convex shapes can be moved or cast. `is_trimesh` lets the world refuse a triangle mesh on
//! a dynamic body.

use super::compound::CompoundShape;
use super::convex::ConvexShape;
use super::trimesh::TriMeshShape;
use crate::PhysicsError;
use engine_assets::format::CollisionShape;
use glam::{Quat, Vec3};
use rapier3d::prelude::SharedShape;

/// Collider geometry; only convex shapes can be moved or cast.
#[derive(Debug, Clone)]
pub enum ColliderShape {
    /// Convex geometry.
    Convex(ConvexShape),
    /// Static or kinematic triangle surface.
    TriMesh(TriMeshShape),
    /// Compound convex geometry.
    Compound(CompoundShape),
}
impl ColliderShape {
    /// Converts one cooked collision record to a shared runtime shape.
    pub fn from_cooked(shape: &CollisionShape) -> Result<Self, PhysicsError> {
        match shape {
            CollisionShape::Box {
                translation,
                rotation,
                half_extents,
            } => Ok(Self::Compound(CompoundShape::new(vec![(
                Vec3::from_array(*translation),
                Quat::from_array(*rotation),
                ConvexShape::cuboid(Vec3::from_array(*half_extents))?,
            )])?)),
            CollisionShape::Hull { points } => Ok(Self::Convex(ConvexShape::hull(points)?)),
            CollisionShape::TriMesh { positions, indices } => {
                if !indices.len().is_multiple_of(3) {
                    return Err(PhysicsError::InvalidMesh);
                }
                let triangles = indices.as_chunks::<3>().0.to_vec();
                Ok(Self::TriMesh(TriMeshShape::new(
                    positions.iter().map(|p| Vec3::from_array(*p)).collect(),
                    triangles,
                )?))
            }
            CollisionShape::Decomposed { hulls } => Ok(Self::Compound(CompoundShape::new(
                hulls
                    .iter()
                    .map(|points| Ok((Vec3::ZERO, Quat::IDENTITY, ConvexShape::hull(points)?)))
                    .collect::<Result<Vec<_>, PhysicsError>>()?,
            )?)),
        }
    }
    pub(crate) fn shared(&self) -> SharedShape {
        match self {
            Self::Convex(shape) => shape.shared(),
            Self::TriMesh(shape) => (*shape.shared).clone(),
            Self::Compound(shape) => shape.shared(),
        }
    }
    pub(crate) const fn is_trimesh(&self) -> bool {
        matches!(self, Self::TriMesh(_))
    }
}
impl From<ConvexShape> for ColliderShape {
    fn from(value: ConvexShape) -> Self {
        Self::Convex(value)
    }
}
