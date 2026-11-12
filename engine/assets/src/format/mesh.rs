//! Mesh records, LOD streams, bounds, and the node/material index relationships they require.
//!
//! Vertex streams remain separate so renderer uploads can bind each format independently.
use serde::{Deserialize, Serialize};

use super::{Clip, Material, Node};

/// Maximum cooked LOD count accepted by the renderer.
pub const MAX_LODS: usize = 4;
pub(super) const MAX_COLLISION_SETS: usize = 65_536;
const MAX_COLLISION_SHAPES_PER_SET: usize = 64;
const MAX_COLLISION_POINTS: usize = 1_000_000;
const MAX_COLLISION_INDICES: usize = 3_000_000;

/// Axis-aligned and sphere bounds for one LOD.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    /// Minimum corner of the box.
    pub min: [f32; 3],
    /// Maximum corner of the box.
    pub max: [f32; 3],
    /// Sphere centre.
    pub center: [f32; 3],
    /// Sphere radius.
    pub radius: f32,
}

/// Position encoding selected for a mesh LOD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionEncoding {
    /// Three u16 coordinates with two reserved zero bytes (8 bytes per vertex).
    Quantized16,
    /// Three little-endian f32 coordinates (12 bytes per vertex).
    Float32,
}

/// One mesh LOD with explicitly separate GPU streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshLod {
    /// Vertex position stream.
    pub positions: Vec<u8>,
    /// Octahedral normal, signed tangent and two half-float UVs per vertex.
    pub attributes: Vec<u8>,
    /// Little-endian indices, selected by [`IndexEncoding`].
    pub indices: Vec<u8>,
    /// Index width.
    pub index_encoding: IndexEncoding,
    /// Position stream encoding.
    pub position_encoding: PositionEncoding,
    /// Bounds for this LOD.
    pub bounds: Bounds,
    /// Screen-size threshold at which this LOD becomes active.
    pub screen_size_threshold: f32,
    /// Relative geometric error returned by meshopt simplification for this LOD.
    pub screen_error: f32,
    /// Number of vertices.
    pub vertex_count: u32,
    /// Number of indices.
    pub index_count: u32,
}

/// Index element width for a mesh LOD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexEncoding {
    /// Unsigned 16-bit indices.
    U16,
    /// Unsigned 32-bit indices.
    U32,
}

/// One mesh and all of its LODs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    /// LODs ordered from highest to lowest detail.
    pub lods: Vec<MeshLod>,
    /// Material index used by this mesh.
    pub material: u32,
    /// Scene node index owning this mesh.
    pub node: u32,
}
/// Engine-ready representation of a cooked mesh asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshPack {
    /// 64-bit deterministic source content hash.
    pub source_hash: u64,
    /// Meshes in source order.
    pub meshes: Vec<Mesh>,
    /// Materials.
    pub materials: Vec<Material>,
    /// Node hierarchy in parent-before-child order.
    pub nodes: Vec<Node>,
    /// Animation clips.
    pub clips: Vec<Clip>,
    /// Physics collision geometry, grouped by scene node.
    pub collision: Vec<CollisionSet>,
    /// Content hashes of separately stored KTX2 textures.
    pub textures: Vec<u64>,
}

/// Physics geometry cooked for one scene node and its material surface name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollisionSet {
    /// Node whose local transform places these shapes.
    pub node: u32,
    /// Material tag used by the game to select a physical surface.
    pub surface: String,
    /// Whether this set is a trigger sensor.
    pub is_sensor: bool,
    /// Physics shapes in node-local coordinates.
    pub shapes: Vec<CollisionShape>,
}

/// One validated-at-decode cooked physics shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CollisionShape {
    /// Oriented box with half extents.
    Box {
        /// Local translation.
        translation: [f32; 3],
        /// Local rotation as x, y, z, w.
        rotation: [f32; 4],
        /// Positive half extents.
        half_extents: [f32; 3],
    },
    /// Convex hull point cloud.
    Hull {
        /// Points defining the hull.
        points: Vec<[f32; 3]>,
    },
    /// Static triangle surface.
    TriMesh {
        /// Vertices in local coordinates.
        positions: Vec<[f32; 3]>,
        /// Triangle indices.
        indices: Vec<u32>,
    },
    /// Cook-time convex decomposition.
    Decomposed {
        /// Convex hulls produced by the cooker.
        hulls: Vec<Vec<[f32; 3]>>,
    },
}
impl MeshPack {
    /// Iterates generic sockets recognized by the `<TAG>_Socket_<Name>` convention.
    pub fn sockets(&self) -> impl Iterator<Item = (&str, usize)> {
        self.nodes.iter().enumerate().filter_map(|(index, node)| {
            node.name
                .split_once("_Socket_")
                .and_then(|(_, name)| (!name.is_empty()).then_some((name, index)))
        })
    }

    /// Computes a node's world transform by composing its parent chain from root to leaf.
    pub fn node_world_transform(&self, node_index: usize) -> Option<glam::Mat4> {
        let node = self.nodes.get(node_index)?;
        let local = |node: &Node| {
            glam::Mat4::from_scale_rotation_translation(
                glam::Vec3::from_array(node.scale),
                glam::Quat::from_xyzw(
                    node.rotation[0],
                    node.rotation[1],
                    node.rotation[2],
                    node.rotation[3],
                ),
                glam::Vec3::from_array(node.translation),
            )
        };
        let mut world = local(node);
        let mut parent = node.parent;
        while let Some(parent_index) = parent {
            let parent_node = self.nodes.get(usize::try_from(parent_index).ok()?)?;
            world = local(parent_node) * world;
            parent = parent_node.parent;
        }
        Some(world)
    }
}

pub(super) fn valid_mesh(mesh: &Mesh, material_count: usize, node_count: usize) -> bool {
    !mesh.lods.is_empty()
        && mesh.lods.len() <= MAX_LODS
        && usize::try_from(mesh.material).is_ok_and(|index| index < material_count)
        && usize::try_from(mesh.node).is_ok_and(|index| index < node_count)
        && mesh.lods.iter().all(valid_lod)
}

pub(super) fn valid_collision(set: &CollisionSet, node_count: usize) -> bool {
    usize::try_from(set.node).is_ok_and(|node| node < node_count)
        && !set.surface.is_empty()
        && !set.shapes.is_empty()
        && set.shapes.len() <= MAX_COLLISION_SHAPES_PER_SET
        && set.shapes.iter().all(valid_collision_shape)
}

fn valid_collision_shape(shape: &CollisionShape) -> bool {
    match shape {
        CollisionShape::Box {
            translation,
            rotation,
            half_extents,
        } => {
            translation.iter().all(|value| value.is_finite())
                && rotation.iter().all(|value| value.is_finite())
                && rotation.iter().map(|value| value * value).sum::<f32>() > f32::EPSILON
                && half_extents
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
        }
        CollisionShape::Hull { points } => valid_hull(points),
        CollisionShape::TriMesh { positions, indices } => {
            positions.len() >= 3
                && positions.len() <= MAX_COLLISION_POINTS
                && !indices.is_empty()
                && indices.len() <= MAX_COLLISION_INDICES
                && indices.len().is_multiple_of(3)
                && positions.iter().flatten().all(|value| value.is_finite())
                && indices
                    .iter()
                    .all(|index| usize::try_from(*index).is_ok_and(|i| i < positions.len()))
                && indices.as_chunks::<3>().0.iter().all(|triangle| {
                    let Some(a) = triangle
                        .first()
                        .and_then(|index| usize::try_from(*index).ok())
                        .and_then(|i| positions.get(i))
                    else {
                        return false;
                    };
                    let Some(b) = triangle
                        .get(1)
                        .and_then(|index| usize::try_from(*index).ok())
                        .and_then(|i| positions.get(i))
                    else {
                        return false;
                    };
                    let Some(c) = triangle
                        .get(2)
                        .and_then(|index| usize::try_from(*index).ok())
                        .and_then(|i| positions.get(i))
                    else {
                        return false;
                    };
                    let ab = glam::Vec3::from_array(*b) - glam::Vec3::from_array(*a);
                    let ac = glam::Vec3::from_array(*c) - glam::Vec3::from_array(*a);
                    ab.cross(ac).length_squared() > 1.0e-12
                })
        }
        CollisionShape::Decomposed { hulls } => {
            !hulls.is_empty() && hulls.len() <= 64 && hulls.iter().all(|hull| valid_hull(hull))
        }
    }
}

fn valid_hull(points: &[[f32; 3]]) -> bool {
    if points.len() < 4
        || points.len() > MAX_COLLISION_POINTS
        || points.iter().flatten().any(|value| !value.is_finite())
    {
        return false;
    }
    let Some(origin_point) = points.first() else {
        return false;
    };
    let origin = glam::Vec3::from_array(*origin_point);
    let first = points
        .iter()
        .skip(1)
        .map(|point| glam::Vec3::from_array(*point) - origin)
        .find(|point| point.length_squared() > 1.0e-12);
    let Some(first) = first else {
        return false;
    };
    let second = points
        .iter()
        .skip(1)
        .map(|point| glam::Vec3::from_array(*point) - origin)
        .find(|point| first.cross(*point).length_squared() > 1.0e-12);
    let Some(second) = second else {
        return false;
    };
    points.iter().skip(1).any(|point| {
        let third = glam::Vec3::from_array(*point) - origin;
        first.dot(second.cross(third)).abs() > 1.0e-9
    })
}

fn valid_lod(lod: &MeshLod) -> bool {
    let vertex_count = usize::try_from(lod.vertex_count).unwrap_or(usize::MAX);
    let index_count = usize::try_from(lod.index_count).unwrap_or(usize::MAX);
    let position_stride = match lod.position_encoding {
        PositionEncoding::Quantized16 => 8,
        PositionEncoding::Float32 => 12,
    };
    let index_stride = match lod.index_encoding {
        IndexEncoding::U16 => 2,
        IndexEncoding::U32 => 4,
    };
    let encoded_indices_valid = match lod.index_encoding {
        IndexEncoding::U16 => lod
            .indices
            .as_chunks::<2>()
            .0
            .iter()
            .all(|bytes| usize::from(u16::from_le_bytes(*bytes)) < vertex_count),
        IndexEncoding::U32 => lod.indices.as_chunks::<4>().0.iter().all(|bytes| {
            usize::try_from(u32::from_le_bytes(*bytes)).is_ok_and(|index| index < vertex_count)
        }),
    };
    lod.vertex_count > 0
        && lod.index_count >= 3
        && lod.index_count.is_multiple_of(3)
        && lod.positions.len() == vertex_count.saturating_mul(position_stride)
        && lod.attributes.len() == vertex_count.saturating_mul(12)
        && lod.indices.len() == index_count.saturating_mul(index_stride)
        && encoded_indices_valid
        && lod
            .bounds
            .min
            .iter()
            .chain(lod.bounds.max.iter())
            .chain(lod.bounds.center.iter())
            .all(|value| value.is_finite())
        && lod.bounds.radius.is_finite()
        && lod.bounds.radius >= 0.0
        && lod.screen_size_threshold.is_finite()
        && lod.screen_size_threshold >= 0.0
        && lod.screen_error.is_finite()
        && lod.screen_error >= 0.0
        && lod
            .bounds
            .min
            .iter()
            .zip(lod.bounds.max.iter())
            .all(|(minimum, maximum)| minimum <= maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lod() -> MeshLod {
        MeshLod {
            positions: vec![0; 36],
            attributes: vec![0; 36],
            indices: [0u16, 1, 2]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            index_encoding: IndexEncoding::U16,
            position_encoding: PositionEncoding::Float32,
            bounds: Bounds {
                min: [0.0; 3],
                max: [1.0; 3],
                center: [0.5; 3],
                radius: 1.0,
            },
            screen_size_threshold: 0.5,
            screen_error: 0.0,
            vertex_count: 3,
            index_count: 3,
        }
    }

    #[test]
    fn mesh_validation_accepts_four_lods_and_rejects_five() {
        let mut mesh = Mesh {
            lods: vec![lod(); MAX_LODS],
            material: 0,
            node: 0,
        };
        assert!(valid_mesh(&mesh, 1, 1));
        mesh.lods.push(lod());
        assert!(!valid_mesh(&mesh, 1, 1));
    }
}
