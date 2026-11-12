//! Builds optimized mesh LODs and their compact GPU streams.
//!
//! Position quantization is selected from a named error bound; each stream remains independently encoded.
use std::collections::BTreeMap;

use engine_assets::format::{Bounds, IndexEncoding, MAX_LODS, Mesh, MeshLod, PositionEncoding};
use gltf::{Document, buffer};
use half::f16;

use crate::CookError;

const TANGENT_EPSILON: f32 = 1.0e-8;
const POSITION_ERROR_LIMIT: f32 = 0.0001;
const LOD_ERROR_LIMIT: f32 = 0.02;
const LOD_TRIANGLE_FRACTIONS: [(usize, usize); 2] = [(1, 2), (1, 4)];
const LOD_SCREEN_THRESHOLDS: [f32; 2] = [0.5, 0.25];

#[derive(Clone, Default)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) tangent: [f32; 4],
    pub(crate) uv: [f32; 2],
}

pub(crate) fn generate_tangents(vertices: &mut [Vertex], indices: &[u32]) -> Result<(), CookError> {
    struct Geometry<'a> {
        vertices: &'a mut [Vertex],
        indices: &'a [u32],
    }
    impl mikktspace::Geometry for Geometry<'_> {
        fn num_faces(&self) -> usize {
            self.indices.len() / 3
        }
        fn num_vertices_of_face(&self, _face: usize) -> usize {
            3
        }
        fn position(&self, face: usize, vert: usize) -> [f32; 3] {
            self.indices
                .get(face * 3 + vert)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| self.vertices.get(index))
                .map_or([0.0; 3], |vertex| vertex.position)
        }
        fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
            self.indices
                .get(face * 3 + vert)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| self.vertices.get(index))
                .map_or([0.0, 1.0, 0.0], |vertex| vertex.normal)
        }
        fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
            self.indices
                .get(face * 3 + vert)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| self.vertices.get(index))
                .map_or([0.0; 2], |vertex| vertex.uv)
        }
        fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
            if let Some(index) = self
                .indices
                .get(face * 3 + vert)
                .and_then(|index| usize::try_from(*index).ok())
                && let Some(vertex) = self.vertices.get_mut(index)
            {
                vertex.tangent = tangent;
            }
        }
    }
    if !indices.len().is_multiple_of(3) {
        return Err(CookError::Mesh);
    }
    let mut geometry = Geometry { vertices, indices };
    if !mikktspace::generate_tangents(&mut geometry) {
        return Err(CookError::Mesh);
    }
    Ok(())
}

fn fetch_optimize(
    vertices: Vec<Vertex>,
    mut indices: Vec<u32>,
) -> Result<(Vec<Vertex>, Vec<u32>), CookError> {
    let remap = meshopt::optimize::optimize_vertex_fetch_remap(&indices, vertices.len());
    if remap.len() != vertices.len() {
        return Err(CookError::Mesh);
    }
    let compact = meshopt::remap::remap_vertex_buffer(&vertices, vertices.len(), &remap);
    indices = meshopt::remap::remap_index_buffer(Some(&indices), vertices.len(), &remap);
    Ok((compact, indices))
}

type LodBuild = (Vec<MeshLod>, Vec<u32>, Vec<[usize; 3]>);

/// Mesh data and report measurements produced by glTF primitive conversion.
pub(super) struct MeshBuild {
    pub(super) meshes: Vec<Mesh>,
    pub(super) triangles_per_lod: Vec<Vec<u32>>,
    pub(super) stream_bytes: Vec<Vec<[usize; 3]>>,
}

fn make_lods(vertices: &[Vertex], indices: &[u32]) -> Result<LodBuild, CookError> {
    let mut result = Vec::new();
    let mut triangles = Vec::new();
    let mut byte_counts = Vec::new();
    let mut current = indices.to_vec();
    let position_buffer = vertices
        .iter()
        .flat_map(|vertex| vertex.position.into_iter().flat_map(f32::to_le_bytes))
        .collect::<Vec<_>>();
    let position_adapter =
        meshopt::VertexDataAdapter::new(&position_buffer, 12, 0).map_err(|_| CookError::Mesh)?;
    let mesh_bounds = bounds(vertices)?;
    let mut screen_error = 0.0;
    for level in 0..=LOD_TRIANGLE_FRACTIONS.len() {
        if result.len() >= MAX_LODS {
            return Err(CookError::Mesh);
        }
        let threshold = if level == 0 {
            1.0
        } else {
            LOD_SCREEN_THRESHOLDS.get(level - 1).copied().unwrap_or(0.0)
        };
        let (lod_vertices, lod_indices) = compact_lod(vertices, &current)?;
        let (lod, counts) = encode_lod(
            &lod_vertices,
            &lod_indices,
            threshold,
            screen_error,
            mesh_bounds,
        )?;
        triangles.push(u32::try_from(current.len() / 3).map_err(|_| CookError::Mesh)?);
        byte_counts.push(counts);
        result.push(lod);
        if level == LOD_TRIANGLE_FRACTIONS.len() || current.len() <= 3 {
            break;
        }
        let fraction = LOD_TRIANGLE_FRACTIONS
            .get(level)
            .copied()
            .ok_or(CookError::Mesh)?;
        let (numerator, denominator) = fraction;
        let target = indices.len().saturating_mul(numerator) / denominator;
        let target = target.max(3).min(current.len().saturating_sub(3));
        let mut simplified_error = 0.0;
        let simplified = meshopt::simplify::simplify(
            &current,
            &position_adapter,
            target,
            LOD_ERROR_LIMIT,
            meshopt::simplify::SimplifyOptions::empty(),
            Some(&mut simplified_error),
        );
        if simplified.len() >= current.len() || simplified.len() < 3 {
            break;
        }
        // Each simplification pass measures error against its input LOD, so accumulate the
        // largest error observed to keep thresholds conservative across the full chain.
        screen_error += simplified_error;
        current = meshopt::optimize::optimize_vertex_cache(&simplified, vertices.len());
    }
    Ok((result, triangles, byte_counts))
}

fn compact_lod(vertices: &[Vertex], indices: &[u32]) -> Result<(Vec<Vertex>, Vec<u32>), CookError> {
    let mut remap = BTreeMap::<u32, u32>::new();
    let mut compact = Vec::new();
    let mut output_indices = Vec::with_capacity(indices.len());
    for index in indices {
        let vertex = vertices
            .get(usize::try_from(*index).map_err(|_| CookError::Mesh)?)
            .ok_or(CookError::Mesh)?;
        let next = u32::try_from(compact.len()).map_err(|_| CookError::Mesh)?;
        let mapped = *remap.entry(*index).or_insert_with(|| {
            compact.push(vertex.clone());
            next
        });
        output_indices.push(mapped);
    }
    Ok((compact, output_indices))
}

pub(crate) fn encode_lod(
    vertices: &[Vertex],
    indices: &[u32],
    threshold: f32,
    screen_error: f32,
    bounds: Bounds,
) -> Result<(MeshLod, [usize; 3]), CookError> {
    let extents = [
        bounds.max[0] - bounds.min[0],
        bounds.max[1] - bounds.min[1],
        bounds.max[2] - bounds.min[2],
    ];
    let error = ((extents[0] * extents[0] + extents[1] * extents[1] + extents[2] * extents[2])
        .sqrt())
        / 131070.0;
    let position_encoding = if error <= POSITION_ERROR_LIMIT {
        PositionEncoding::Quantized16
    } else {
        PositionEncoding::Float32
    };
    let mut positions = Vec::with_capacity(
        vertices.len()
            * if position_encoding == PositionEncoding::Quantized16 {
                8
            } else {
                12
            },
    );
    let mut attributes = Vec::with_capacity(vertices.len() * 12);
    for vertex in vertices {
        if position_encoding == PositionEncoding::Quantized16 {
            for axis in 0..3 {
                let extent = extents.get(axis).copied().ok_or(CookError::Mesh)?;
                let minimum = bounds.min.get(axis).copied().ok_or(CookError::Mesh)?;
                let normalized = if extent == 0.0 {
                    0.0
                } else {
                    ((vertex.position.get(axis).copied().ok_or(CookError::Mesh)? - minimum)
                        / extent)
                        .clamp(0.0, 1.0)
                };
                let quantized = meshopt::quantize_unorm(normalized, 16);
                positions.extend_from_slice(
                    &u16::try_from(quantized)
                        .map_err(|_| CookError::Mesh)?
                        .to_le_bytes(),
                );
            }
            positions.extend_from_slice(&[0, 0]);
        } else {
            for value in vertex.position {
                positions.extend_from_slice(&value.to_le_bytes());
            }
        }
        let normal = oct_encode(vertex.normal);
        attributes.extend_from_slice(&normal.0.to_le_bytes());
        attributes.extend_from_slice(&normal.1.to_le_bytes());
        let tangent = oct_encode([vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]]);
        let tangent_x = meshopt::quantize_unorm(f32::from(tangent.0) / 32767.0 * 0.5 + 0.5, 15);
        let tangent_y = meshopt::quantize_unorm(f32::from(tangent.1) / 32767.0 * 0.5 + 0.5, 15);
        let handedness = u32::from(vertex.tangent[3] < 0.0);
        let tangent_x = u32::try_from(tangent_x).map_err(|_| CookError::Mesh)?;
        let tangent_y = u32::try_from(tangent_y).map_err(|_| CookError::Mesh)?;
        let packed_tangent = tangent_x | (tangent_y << 15) | (handedness << 30);
        attributes.extend_from_slice(&packed_tangent.to_le_bytes());
        attributes.extend_from_slice(&f16::from_f32(vertex.uv[0]).to_bits().to_le_bytes());
        attributes.extend_from_slice(&f16::from_f32(vertex.uv[1]).to_bits().to_le_bytes());
    }
    let index_encoding = if vertices.len() < 65_536 {
        IndexEncoding::U16
    } else {
        IndexEncoding::U32
    };
    let mut encoded_indices = Vec::new();
    for index in indices {
        if index_encoding == IndexEncoding::U16 {
            encoded_indices.extend_from_slice(
                &u16::try_from(*index)
                    .map_err(|_| CookError::Mesh)?
                    .to_le_bytes(),
            );
        } else {
            encoded_indices.extend_from_slice(&index.to_le_bytes());
        }
    }
    let counts = [positions.len(), attributes.len(), encoded_indices.len()];
    Ok((
        MeshLod {
            positions,
            attributes,
            indices: encoded_indices,
            index_encoding,
            position_encoding,
            bounds,
            screen_size_threshold: threshold,
            screen_error,
            vertex_count: u32::try_from(vertices.len()).map_err(|_| CookError::Mesh)?,
            index_count: u32::try_from(indices.len()).map_err(|_| CookError::Mesh)?,
        },
        counts,
    ))
}

fn bounds(vertices: &[Vertex]) -> Result<Bounds, CookError> {
    if vertices.iter().any(|vertex| {
        vertex
            .position
            .iter()
            .chain(vertex.normal.iter())
            .chain(vertex.tangent.iter())
            .chain(vertex.uv.iter())
            .any(|value| !value.is_finite())
            || vertex.normal.iter().map(|value| value * value).sum::<f32>() <= TANGENT_EPSILON
            || vertex.uv.iter().any(|value| value.abs() > 65504.0)
    }) {
        return Err(CookError::Mesh);
    }
    let first = vertices.first().ok_or(CookError::Mesh)?;
    let mut min = first.position;
    let mut max = first.position;
    for vertex in vertices.iter().skip(1) {
        for ((minimum, maximum), coordinate) in
            min.iter_mut().zip(max.iter_mut()).zip(vertex.position)
        {
            *minimum = minimum.min(coordinate);
            *maximum = maximum.max(coordinate);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let radius = vertices
        .iter()
        .map(|vertex| {
            let dx = vertex.position[0] - center[0];
            let dy = vertex.position[1] - center[1];
            let dz = vertex.position[2] - center[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .fold(0.0, f32::max);
    Ok(Bounds {
        min,
        max,
        center,
        radius,
    })
}

fn oct_encode(mut vector: [f32; 3]) -> (i16, i16) {
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
        .sqrt()
        .max(TANGENT_EPSILON);
    for value in &mut vector {
        *value /= length;
    }
    let denominator = vector[0].abs() + vector[1].abs() + vector[2].abs();
    let mut x = vector[0] / denominator;
    let mut y = vector[1] / denominator;
    if vector[2] < 0.0 {
        let old_x = x;
        x = (1.0 - y.abs()) * old_x.signum();
        y = (1.0 - old_x.abs()) * y.signum();
    }
    (
        num_traits::ToPrimitive::to_i16(&(x.clamp(-1.0, 1.0) * 32767.0).round()).unwrap_or(0),
        num_traits::ToPrimitive::to_i16(&(y.clamp(-1.0, 1.0) * 32767.0).round()).unwrap_or(0),
    )
}
fn expand_triangles(
    vertices: Vec<Vertex>,
    indices: &[u32],
) -> Result<(Vec<Vertex>, Vec<u32>), CookError> {
    if !indices.len().is_multiple_of(3) {
        return Err(CookError::Mesh);
    }
    let mut expanded = Vec::with_capacity(indices.len());
    let mut expanded_indices = Vec::with_capacity(indices.len());
    for index in indices {
        let source = usize::try_from(*index).map_err(|_| CookError::Mesh)?;
        expanded.push(vertices.get(source).cloned().ok_or(CookError::Mesh)?);
        expanded_indices.push(u32::try_from(expanded_indices.len()).map_err(|_| CookError::Mesh)?);
    }
    Ok((expanded, expanded_indices))
}

fn weld_vertices(
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
) -> Result<(Vec<Vertex>, Vec<u32>), CookError> {
    let mut remapped = BTreeMap::<[u32; 12], u32>::new();
    let mut welded = Vec::new();
    let mut output_indices = Vec::with_capacity(indices.len());
    for index in indices {
        let vertex = vertices
            .get(usize::try_from(index).map_err(|_| CookError::Mesh)?)
            .ok_or(CookError::Mesh)?;
        let key = [
            vertex.position[0].to_bits(),
            vertex.position[1].to_bits(),
            vertex.position[2].to_bits(),
            vertex.normal[0].to_bits(),
            vertex.normal[1].to_bits(),
            vertex.normal[2].to_bits(),
            vertex.tangent[0].to_bits(),
            vertex.tangent[1].to_bits(),
            vertex.tangent[2].to_bits(),
            vertex.tangent[3].to_bits(),
            vertex.uv[0].to_bits(),
            vertex.uv[1].to_bits(),
        ];
        let next = u32::try_from(welded.len()).map_err(|_| CookError::Mesh)?;
        let mapped = *remapped.entry(key).or_insert_with(|| {
            welded.push(vertex.clone());
            next
        });
        output_indices.push(mapped);
    }
    Ok((welded, output_indices))
}

/// Reads, optimizes, and encodes all scene meshes from a glTF document.
pub(super) fn read_meshes(
    document: &Document,
    buffers: &[buffer::Data],
    node_map: &BTreeMap<usize, u32>,
) -> Result<MeshBuild, CookError> {
    let mut meshes = Vec::new();
    let mut triangles_per_lod = Vec::new();
    let mut stream_bytes = Vec::new();
    for node in document.nodes() {
        let Some(mesh_ref) = node.mesh() else {
            continue;
        };
        let node_index = *node_map.get(&node.index()).ok_or(CookError::Mesh)?;
        for primitive in mesh_ref.primitives() {
            let reader = primitive
                .reader(|buffer| buffers.get(buffer.index()).map(|item| item.0.as_slice()));
            let positions = reader
                .read_positions()
                .ok_or(CookError::Mesh)?
                .collect::<Vec<_>>();
            let normals = reader
                .read_normals()
                .map(|iter| iter.collect::<Vec<_>>())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let uvs = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().collect::<Vec<_>>())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let indices = reader
                .read_indices()
                .map(|iter| iter.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..u32::try_from(positions.len()).unwrap_or(0)).collect());
            let tangents = reader.read_tangents().map(|iter| iter.collect::<Vec<_>>());
            let vertices = positions
                .iter()
                .enumerate()
                .map(|(index, position)| Vertex {
                    position: *position,
                    normal: normals.get(index).copied().unwrap_or([0.0, 1.0, 0.0]),
                    tangent: tangents
                        .as_ref()
                        .and_then(|values| values.get(index))
                        .copied()
                        .unwrap_or([1.0, 0.0, 0.0, 1.0]),
                    uv: uvs.get(index).copied().unwrap_or([0.0, 0.0]),
                })
                .collect::<Vec<_>>();
            let (mut vertices, indices) = if tangents.is_none() {
                expand_triangles(vertices, &indices)?
            } else {
                (vertices, indices)
            };
            if tangents.is_none() {
                generate_tangents(&mut vertices, &indices)?;
            }
            let (vertices, indices) = weld_vertices(vertices, indices)?;
            let indices = meshopt::optimize::optimize_vertex_cache(&indices, vertices.len());
            let (vertices, indices) = fetch_optimize(vertices, indices)?;
            let material = u32::try_from(primitive.material().index().unwrap_or(0))
                .map_err(|_| CookError::Mesh)?;
            let (lods, tris, streams) = make_lods(&vertices, &indices)?;
            meshes.push(Mesh {
                lods,
                material,
                node: node_index,
            });
            triangles_per_lod.push(tris);
            stream_bytes.push(streams);
        }
    }
    Ok(MeshBuild {
        meshes,
        triangles_per_lod,
        stream_bytes,
    })
}
