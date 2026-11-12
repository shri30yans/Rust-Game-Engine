//! Selects and cooks physics geometry from generic Blender node tags.
//!
//! Collision vertices are kept in the owning node's local space; decomposition is completed here
//! so runtime physics only creates already cooked convex shapes.

use std::collections::BTreeMap;

use engine_assets::format::{CollisionSet, CollisionShape, Node};
use engine_physics::{ConvexShape, DecompositionParams, decompose_mesh};
use glam::Vec3;
use gltf::{Document, buffer};

use crate::CookError;

const DEFAULT_RESOLUTION: u32 = 64;
const DEFAULT_MAX_HULLS: u32 = 16;
const DEFAULT_CONCAVITY: f32 = 0.0025;
const CONVEXITY_VOLUME_GAP: f32 = 0.02;

/// Collision records and objects that had no recognized collision role.
pub(super) struct CollisionBuild {
    pub(super) sets: Vec<CollisionSet>,
    pub(super) uncooked_objects: Vec<String>,
}

/// Reads collision tags and cooks role-based shapes from source LOD0 geometry.
pub(super) fn cook(
    document: &Document,
    buffers: &[buffer::Data],
    node_map: &BTreeMap<usize, u32>,
    nodes: &[Node],
) -> Result<CollisionBuild, CookError> {
    let mut sets = Vec::new();
    let mut uncooked_objects = Vec::new();
    for object in document.nodes() {
        let object_name = object.name().unwrap_or("<unnamed>");
        let Some(&owner_index) = node_map.get(&object.index()) else {
            continue;
        };
        let role = tag(
            nodes.get(usize::try_from(owner_index).map_err(|_| CookError::Mesh)?),
            "role",
        );
        let collision_child = object.children().find(|child| {
            child
                .name()
                .is_some_and(|name| name == format!("{object_name}_COL"))
        });
        if let Some(child) = collision_child {
            let kind = tag_by_gltf(child.extras().as_deref(), "collision")
                .unwrap_or_else(|| "hull".to_owned());
            let geometry = geometry(child.clone(), buffers, true)?;
            let params = decomposition_params(
                object_name,
                nodes.get(usize::try_from(owner_index).map_err(|_| CookError::Mesh)?),
            )?;
            let shape = cook_tagged_shape(object_name, &kind, geometry.0, geometry.1, params)?;
            sets.push(CollisionSet {
                node: owner_index,
                surface: material(
                    nodes.get(usize::try_from(owner_index).map_err(|_| CookError::Mesh)?),
                ),
                is_sensor: role == Some("trigger"),
                shapes: vec![shape],
            });
            continue;
        }
        let Some(role) = role else {
            if object.mesh().is_some() {
                uncooked_objects.push(object_name.to_owned());
            }
            continue;
        };
        if !matches!(role, "static" | "prop" | "mover" | "breakable" | "trigger") {
            continue;
        }
        let (vertices, indices) = geometry(object, buffers, false)?;
        let shape = match role {
            "static" => CollisionShape::TriMesh {
                positions: positions(&vertices),
                indices: indices.into_iter().flatten().collect(),
            },
            "trigger" => box_shape(&vertices)?,
            _ => {
                let params = decomposition_params(
                    object_name,
                    nodes.get(usize::try_from(owner_index).map_err(|_| CookError::Mesh)?),
                )?;
                if is_convex(&vertices, &indices)? {
                    CollisionShape::Hull {
                        points: positions(&vertices),
                    }
                } else {
                    let hulls = decompose_mesh(&vertices, &indices, params).map_err(|error| {
                        CookError::Collision {
                            object: object_name.to_owned(),
                            reason: error.to_string(),
                        }
                    })?;
                    CollisionShape::Decomposed { hulls }
                }
            }
        };
        sets.push(CollisionSet {
            node: owner_index,
            surface: material(
                nodes.get(usize::try_from(owner_index).map_err(|_| CookError::Mesh)?),
            ),
            is_sensor: role == "trigger",
            shapes: vec![shape],
        });
    }
    Ok(CollisionBuild {
        sets,
        uncooked_objects,
    })
}

fn tag<'a>(node: Option<&'a Node>, key: &str) -> Option<&'a str> {
    node?
        .tags
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn material(node: Option<&Node>) -> String {
    tag(node, "material").unwrap_or("default").to_owned()
}

fn tag_by_gltf(extras: Option<&gltf::json::extras::RawValue>, key: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(extras?.get()).ok()?;
    Some(value.get(key)?.as_str()?.to_owned())
}

fn geometry(
    node: gltf::Node<'_>,
    buffers: &[buffer::Data],
    apply_local_transform: bool,
) -> Result<(Vec<Vec3>, Vec<[u32; 3]>), CookError> {
    let Some(mesh) = node.mesh() else {
        return Err(CookError::Collision {
            object: node.name().unwrap_or("<unnamed>").to_owned(),
            reason: "node has no mesh".to_owned(),
        });
    };
    let matrix = if apply_local_transform {
        glam::Mat4::from_cols_array_2d(&node.transform().matrix())
    } else {
        glam::Mat4::IDENTITY
    };
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for primitive in mesh.primitives() {
        let reader =
            primitive.reader(|buffer| buffers.get(buffer.index()).map(|item| item.0.as_slice()));
        let positions = reader
            .read_positions()
            .ok_or(CookError::Mesh)?
            .collect::<Vec<_>>();
        let offset = u32::try_from(vertices.len()).map_err(|_| CookError::Mesh)?;
        vertices.extend(
            positions
                .into_iter()
                .map(|point| matrix.transform_point3(Vec3::from_array(point))),
        );
        let indices = reader
            .read_indices()
            .map(|iter| iter.into_u32().collect::<Vec<_>>())
            .unwrap_or_else(|| {
                (0..u32::try_from(vertices.len())
                    .unwrap_or(0)
                    .saturating_sub(offset))
                    .collect()
            });
        if !indices.len().is_multiple_of(3) {
            return Err(CookError::Mesh);
        }
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = *triangle;
            let a = a.checked_add(offset).ok_or(CookError::Mesh)?;
            let b = b.checked_add(offset).ok_or(CookError::Mesh)?;
            let c = c.checked_add(offset).ok_or(CookError::Mesh)?;
            triangles.push([a, b, c]);
        }
    }
    if vertices.len() < 3
        || triangles.is_empty()
        || vertices.iter().any(|vertex| !vertex.is_finite())
    {
        return Err(CookError::Mesh);
    }
    Ok((vertices, triangles))
}

fn positions(vertices: &[Vec3]) -> Vec<[f32; 3]> {
    vertices.iter().map(|point| point.to_array()).collect()
}

fn cook_tagged_shape(
    object: &str,
    kind: &str,
    vertices: Vec<Vec3>,
    indices: Vec<[u32; 3]>,
    params: DecompositionParams,
) -> Result<CollisionShape, CookError> {
    match kind {
        "box" => box_shape(&vertices),
        "hull" => Ok(CollisionShape::Hull {
            points: positions(&vertices),
        }),
        "mesh" => Ok(CollisionShape::TriMesh {
            positions: positions(&vertices),
            indices: indices.into_iter().flatten().collect(),
        }),
        "decompose" => Ok(CollisionShape::Decomposed {
            hulls: decompose_mesh(&vertices, &indices, params).map_err(|error| {
                CookError::Collision {
                    object: object.to_owned(),
                    reason: error.to_string(),
                }
            })?,
        }),
        _ => Err(CookError::Collision {
            object: object.to_owned(),
            reason: format!("unknown collision tag `{kind}`"),
        }),
    }
}

fn box_shape(vertices: &[Vec3]) -> Result<CollisionShape, CookError> {
    let first = vertices.first().copied().ok_or(CookError::Mesh)?;
    let (mut minimum, mut maximum) = (first, first);
    for point in vertices.iter().copied().skip(1) {
        minimum = minimum.min(point);
        maximum = maximum.max(point);
    }
    let center = (minimum + maximum) * 0.5;
    let half = (maximum - minimum) * 0.5;
    Ok(CollisionShape::Box {
        translation: center.to_array(),
        rotation: [0.0, 0.0, 0.0, 1.0],
        half_extents: half.max(Vec3::splat(0.001)).to_array(),
    })
}

fn is_convex(vertices: &[Vec3], indices: &[[u32; 3]]) -> Result<bool, CookError> {
    let mut edges = BTreeMap::<(u32, u32), (u32, u32)>::new();
    let mut signed_volume = 0.0_f64;
    for triangle in indices {
        let [a, b, c] = *triangle;
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let (edge, forward) = if from < to {
                ((from, to), true)
            } else {
                ((to, from), false)
            };
            let counts = edges.entry(edge).or_default();
            if forward {
                counts.0 = counts.0.saturating_add(1);
            } else {
                counts.1 = counts.1.saturating_add(1);
            }
        }
        let a = vertices
            .get(usize::try_from(a).map_err(|_| CookError::Mesh)?)
            .ok_or(CookError::Mesh)?;
        let b = vertices
            .get(usize::try_from(b).map_err(|_| CookError::Mesh)?)
            .ok_or(CookError::Mesh)?;
        let c = vertices
            .get(usize::try_from(c).map_err(|_| CookError::Mesh)?)
            .ok_or(CookError::Mesh)?;
        signed_volume += f64::from(a.dot(b.cross(*c))) / 6.0;
    }
    if edges
        .values()
        .any(|(forward, backward)| *forward != 1 || *backward != 1)
    {
        return Ok(false);
    }
    let mesh_volume = signed_volume.abs();
    if mesh_volume <= f64::EPSILON {
        return Ok(false);
    }
    let hull = ConvexShape::hull(&positions(vertices)).map_err(|_| CookError::Mesh)?;
    Ok(f64::from(hull.volume()) <= mesh_volume * (1.0 + f64::from(CONVEXITY_VOLUME_GAP)))
}

fn decomposition_params(
    object: &str,
    node: Option<&Node>,
) -> Result<DecompositionParams, CookError> {
    let resolution = parse_override(object, node, "vhacd_resolution", DEFAULT_RESOLUTION)?;
    let max_hulls = parse_override(object, node, "vhacd_max_hulls", DEFAULT_MAX_HULLS)?;
    DecompositionParams::new(resolution, max_hulls, DEFAULT_CONCAVITY).map_err(|error| {
        CookError::Collision {
            object: object.to_owned(),
            reason: error.to_string(),
        }
    })
}

fn parse_override(
    object: &str,
    node: Option<&Node>,
    key: &str,
    default: u32,
) -> Result<u32, CookError> {
    let Some(value) = tag(node, key) else {
        return Ok(default);
    };
    value.parse::<u32>().map_err(|_| CookError::Collision {
        object: object.to_owned(),
        reason: format!("`{key}` must be an integer"),
    })
}
