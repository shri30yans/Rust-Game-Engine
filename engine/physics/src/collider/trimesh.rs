//! Validated triangle meshes for static and kinematic surfaces.
//!
//! Construction rejects non-finite vertices, too few vertices, empty index lists and degenerate
//! triangles before Rapier sees them. Duplicate vertices are merged and internal edges are fixed.

use std::sync::Arc;

use crate::PhysicsError;
use glam::Vec3;
use rapier3d::prelude::{SharedShape, TriMeshFlags};

/// A triangle mesh validated before it is handed to Rapier.
#[derive(Debug, Clone)]
pub struct TriMeshShape {
    pub(super) shared: Arc<SharedShape>,
}
impl TriMeshShape {
    /// Builds a mesh with merged duplicate vertices and fixed internal edges.
    pub fn new(vertices: Vec<Vec3>, indices: Vec<[u32; 3]>) -> Result<Self, PhysicsError> {
        if vertices.iter().any(|point| !point.is_finite()) {
            return Err(PhysicsError::NonFinite {
                field: "triangle mesh vertices",
            });
        }
        if vertices.len() < 3 || indices.is_empty() {
            return Err(PhysicsError::InvalidMesh);
        }
        for tri in &indices {
            let Some(a) = tri
                .first()
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|i| vertices.get(i))
            else {
                return Err(PhysicsError::InvalidMesh);
            };
            let Some(b) = tri
                .get(1)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|i| vertices.get(i))
            else {
                return Err(PhysicsError::InvalidMesh);
            };
            let Some(c) = tri
                .get(2)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|i| vertices.get(i))
            else {
                return Err(PhysicsError::InvalidMesh);
            };
            if (*b - *a).cross(*c - *a).length_squared() <= 1.0e-12 {
                return Err(PhysicsError::DegenerateTriangle);
            }
        }
        let points: Vec<rapier3d::math::Vector> = vertices
            .into_iter()
            .map(crate::convert::to_rapier_vec)
            .collect();
        let shape = SharedShape::trimesh_with_flags(
            points,
            indices,
            TriMeshFlags::FIX_INTERNAL_EDGES | TriMeshFlags::MERGE_DUPLICATE_VERTICES,
        )
        .map_err(|_| PhysicsError::InvalidMesh)?;
        Ok(Self {
            shared: Arc::new(shape),
        })
    }
}
