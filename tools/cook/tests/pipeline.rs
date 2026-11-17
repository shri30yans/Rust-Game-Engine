//! End-to-end importer checks against caller-supplied pack fixtures in the repository.
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cook::cook_directory;
use engine_assets::format::{self, Asset, MeshLod, PositionEncoding};

const FIXTURE_NAMES: [&str; 2] = ["pistol.glb", "raven7_mobile.glb"];

#[test]
fn pistol_and_mobile_fixture_cook_deterministically_with_documented_data()
-> Result<(), Box<dyn Error>> {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture_root = repository.join("games/imagine/content/weapons_batch1/export");
    let test_root = repository.join("target/cook-pipeline-fixture");
    let input = test_root.join("input");
    let first_output = test_root.join("first");
    let second_output = test_root.join("second");
    let _ = fs::remove_dir_all(&test_root);
    fs::create_dir_all(&input)?;
    for name in FIXTURE_NAMES {
        fs::copy(fixture_root.join(name), input.join(name))?;
    }

    cook_directory(&input, &first_output)?;
    cook_directory(&input, &second_output)?;
    let fresh_bytes = directory_bytes(&first_output)?;
    assert_eq!(fresh_bytes, directory_bytes(&second_output)?);
    cook_directory(&input, &first_output)?;
    assert_eq!(fresh_bytes, directory_bytes(&first_output)?);

    let cooked = fs::read(first_output.join("pistol.meshpack"))?;
    let (_, Asset::MeshPack(pack)) = format::decode(&cooked)?;
    let socket_names = pack.sockets().map(|(name, _)| name).collect::<Vec<_>>();
    for socket in ["Eject", "MagWell", "Muzzle", "Sight"] {
        assert!(socket_names.contains(&socket));
    }
    let (source_document, _, _) = gltf::import(input.join("pistol.glb"))?;
    let mut source_world = std::collections::BTreeMap::new();
    if let Some(scene) = source_document
        .default_scene()
        .or_else(|| source_document.scenes().next())
    {
        for root in scene.nodes() {
            collect_world_transforms(root, glam::Mat4::IDENTITY, &mut source_world)?;
        }
    }
    for (_, node_index) in pack.sockets() {
        let node = pack
            .nodes
            .get(node_index)
            .ok_or_else(|| io::Error::other("socket node missing"))?;
        let transform = pack
            .node_world_transform(node_index)
            .ok_or_else(|| io::Error::other("socket node has no world transform"))?;
        let expected = source_world
            .get(&node.name)
            .ok_or_else(|| io::Error::other("source socket node missing"))?;
        assert!(transform.is_finite());
        assert!(
            transform
                .to_cols_array()
                .iter()
                .zip(expected.to_cols_array().iter())
                .all(|(left, right)| (left - right).abs() < 0.00001)
        );
    }
    let fire = pack
        .clips
        .iter()
        .find(|clip| clip.name == "Fire")
        .ok_or_else(|| io::Error::other("Fire clip is missing"))?;
    assert!((fire.duration - 0.30).abs() < 0.0001);
    assert!(fire.channels.len() >= 3);
    assert!(pack.meshes.iter().flat_map(|mesh| &mesh.lods).all(
        |lod| match lod.position_encoding {
            PositionEncoding::Quantized16 =>
                lod.positions.len() == usize::try_from(lod.vertex_count).unwrap_or(0) * 8,
            PositionEncoding::Float32 =>
                lod.positions.len() == usize::try_from(lod.vertex_count).unwrap_or(0) * 12,
        }
    ));
    for mesh in &pack.meshes {
        assert!(mesh.lods.first().is_some_and(|lod| lod.screen_error == 0.0));
        assert!(
            mesh.lods
                .windows(2)
                .all(|pair| pair.first().zip(pair.get(1)).is_some_and(
                    |(earlier_lod, later_lod)| {
                        earlier_lod.screen_error <= later_lod.screen_error
                    }
                ))
        );
    }
    let report = fs::read_to_string(first_output.join("pistol.report.ron"))?;
    assert!(report.contains("texture_bytes_by_family"));
    assert!(report.contains("0.5") && report.contains("0.25"));
    let mobile_report = fs::read_to_string(first_output.join("raven7_mobile.report.ron"))?;
    assert!(mobile_report.contains("Bc7") && mobile_report.contains("Rgba8"));
    assert_source_geometry_round_trips(&input.join("pistol.glb"), &pack)?;
    fs::remove_dir_all(test_root)?;
    Ok(())
}

fn collect_world_transforms(
    node: gltf::Node<'_>,
    parent: glam::Mat4,
    output: &mut std::collections::BTreeMap<String, glam::Mat4>,
) -> Result<(), Box<dyn Error>> {
    let (translation, rotation, scale) = node.transform().decomposed();
    let local = glam::Mat4::from_scale_rotation_translation(
        glam::Vec3::from_array(scale),
        glam::Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
        glam::Vec3::from_array(translation),
    );
    let world = parent * local;
    output.insert(node.name().unwrap_or("").to_owned(), world);
    for child in node.children() {
        collect_world_transforms(child, world, output)?;
    }
    Ok(())
}

fn assert_source_geometry_round_trips(
    source: &Path,
    pack: &engine_assets::MeshPack,
) -> Result<(), Box<dyn Error>> {
    let (document, buffers, _) = gltf::import(source)?;
    let mut mesh_index = 0usize;
    for node in document.nodes() {
        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let reader = primitive
                .reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));
            let positions = reader
                .read_positions()
                .ok_or_else(|| io::Error::other("positions missing"))?
                .collect::<Vec<_>>();
            let normals = reader
                .read_normals()
                .ok_or_else(|| io::Error::other("normals missing"))?
                .collect::<Vec<_>>();
            let source_indices = reader
                .read_indices()
                .map(|iter| iter.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..u32::try_from(positions.len()).unwrap_or(0)).collect());
            let cooked_mesh = pack
                .meshes
                .get(mesh_index)
                .ok_or_else(|| io::Error::other("cooked mesh missing"))?;
            let lod = cooked_mesh
                .lods
                .first()
                .ok_or_else(|| io::Error::other("LOD0 missing"))?;
            let cooked_positions = decode_positions(lod);
            let cooked_normals = decode_normals(lod);
            let cooked_tangents = decode_tangents(lod);
            for (position, normal) in positions.iter().zip(normals.iter()) {
                let nearest = cooked_positions
                    .iter()
                    .zip(cooked_normals.iter())
                    .filter(|(cooked_position, _)| {
                        distance_squared(**cooked_position, *position)
                            <= 0.0001_f32.powi(2) + 1.0e-10
                    })
                    .map(|(cooked_position, cooked_normal)| {
                        (
                            distance_squared(*cooked_position, *position).sqrt(),
                            dot3(*cooked_normal, *normal),
                        )
                    })
                    .min_by(|left, right| left.0.total_cmp(&right.0))
                    .ok_or_else(|| io::Error::other("source position missing from LOD0"))?;
                let error = nearest.0;
                assert!(
                    error <= 0.0001 + 1.0e-6,
                    "position error exceeded 0.1 mm: {error}"
                );
                let dot = cooked_positions
                    .iter()
                    .zip(cooked_normals.iter())
                    .filter(|(cooked_position, _)| {
                        distance_squared(**cooked_position, *position)
                            <= 0.0001_f32.powi(2) + 1.0e-10
                    })
                    .map(|(_, cooked_normal)| dot3(*cooked_normal, *normal))
                    .max_by(f32::total_cmp)
                    .unwrap_or(nearest.1);
                assert!(dot >= 0.9998, "normal error exceeded one degree: {dot}");
            }
            assert!(cooked_tangents.iter().all(|tangent| {
                let xyz = [tangent[0], tangent[1], tangent[2]];
                let length = dot3(xyz, xyz).sqrt();
                (length - 1.0).abs() < 0.001 && (tangent[3] == 1.0 || tangent[3] == -1.0)
            }));
            let cooked_indices = decode_indices(lod);
            let source_triangles = canonical_triangles(&positions, &source_indices)?;
            let cooked_triangles = canonical_triangles(&cooked_positions, &cooked_indices)?;
            assert_eq!(source_triangles.len(), cooked_triangles.len());
            let mut matched = vec![false; cooked_triangles.len()];
            for source in &source_triangles {
                let Some((index, _)) =
                    cooked_triangles.iter().enumerate().find(|(index, cooked)| {
                        !matched.get(*index).copied().unwrap_or(true)
                            && triangles_match(source, cooked)
                    })
                else {
                    return Err(io::Error::other("cooked triangle set differs from source").into());
                };
                if let Some(is_matched) = matched.get_mut(index) {
                    *is_matched = true;
                }
            }
            mesh_index = mesh_index.saturating_add(1);
        }
    }
    assert_eq!(mesh_index, pack.meshes.len());
    Ok(())
}

fn decode_positions(lod: &MeshLod) -> Vec<[f32; 3]> {
    let stride = if lod.position_encoding == PositionEncoding::Quantized16 {
        8
    } else {
        12
    };
    lod.positions
        .chunks_exact(stride)
        .map(|bytes| {
            std::array::from_fn(|axis| {
                let (value, offset) = match lod.position_encoding {
                    PositionEncoding::Quantized16 => {
                        let start = axis * 2;
                        let raw = bytes
                            .get(start..start + 2)
                            .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
                            .map(u16::from_le_bytes)
                            .unwrap_or(0);
                        let minimum = lod.bounds.min.get(axis).copied().unwrap_or(0.0);
                        let extent = lod.bounds.max.get(axis).copied().unwrap_or(0.0) - minimum;
                        (f32::from(raw) / 65535.0 * extent, minimum)
                    }
                    PositionEncoding::Float32 => {
                        let start = axis * 4;
                        let raw = bytes
                            .get(start..start + 4)
                            .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
                            .map(f32::from_le_bytes)
                            .unwrap_or(0.0);
                        (raw, 0.0)
                    }
                };
                value + offset
            })
        })
        .collect()
}

fn decode_indices(lod: &MeshLod) -> Vec<u32> {
    match lod.index_encoding {
        engine_assets::format::IndexEncoding::U16 => lod
            .indices
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
            .map(u32::from)
            .collect(),
        engine_assets::format::IndexEncoding::U32 => lod
            .indices
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect(),
    }
}

fn canonical_triangles(
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Result<Vec<[[f32; 3]; 3]>, Box<dyn Error>> {
    let (triangles, _) = indices.as_chunks::<3>();
    if indices.len() != triangles.len().saturating_mul(3) {
        return Err(io::Error::other("non-triangle index count").into());
    }
    let mut result = Vec::with_capacity(triangles.len());
    for triangle in triangles {
        let mut points = triangle
            .map(|index| {
                positions
                    .get(usize::try_from(index).unwrap_or(usize::MAX))
                    .copied()
                    .ok_or_else(|| io::Error::other("triangle index out of bounds"))
            })
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        points.sort_by(compare_points);
        let points: [[f32; 3]; 3] = points
            .try_into()
            .map_err(|_| io::Error::other("triangle has wrong point count"))?;
        result.push(points);
    }
    result.sort_by(|left, right| {
        left.iter()
            .zip(right.iter())
            .map(|(a, b)| compare_points(a, b))
            .find(|ordering| !ordering.is_eq())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(result)
}

fn compare_points(left: &[f32; 3], right: &[f32; 3]) -> std::cmp::Ordering {
    left.iter()
        .zip(right.iter())
        .map(|(a, b)| a.total_cmp(b))
        .find(|ordering| !ordering.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn dot3(left: [f32; 3], right: [f32; 3]) -> f32 {
    left.iter().zip(right.iter()).map(|(a, b)| a * b).sum()
}

fn decode_normals(lod: &MeshLod) -> Vec<[f32; 3]> {
    lod.attributes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|bytes| {
            let x = bytes
                .get(0..2)
                .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
                .map(i16::from_le_bytes)
                .unwrap_or(0);
            let y = bytes
                .get(2..4)
                .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
                .map(i16::from_le_bytes)
                .unwrap_or(0);
            decode_octahedral([f32::from(x) / 32767.0, f32::from(y) / 32767.0])
        })
        .collect()
}

fn decode_tangents(lod: &MeshLod) -> Vec<[f32; 4]> {
    lod.attributes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|bytes| {
            let packed = bytes
                .get(4..8)
                .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
                .map(u32::from_le_bytes)
                .unwrap_or(0);
            let x = (packed & 0x7fff) as f32 / 32767.0 * 2.0 - 1.0;
            let y = ((packed >> 15) & 0x7fff) as f32 / 32767.0 * 2.0 - 1.0;
            let tangent = decode_octahedral([x, y]);
            [
                tangent[0],
                tangent[1],
                tangent[2],
                if (packed >> 30) & 1 == 1 { -1.0 } else { 1.0 },
            ]
        })
        .collect()
}

fn decode_octahedral(encoded: [f32; 2]) -> [f32; 3] {
    let mut value = [
        encoded[0],
        encoded[1],
        1.0 - encoded[0].abs() - encoded[1].abs(),
    ];
    if value[2] < 0.0 {
        let x = value[0];
        value[0] = (1.0 - value[1].abs()) * x.signum();
        value[1] = (1.0 - x.abs()) * value[1].signum();
    }
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    [value[0] / length, value[1] / length, value[2] / length]
}

fn distance_squared(left: [f32; 3], right: [f32; 3]) -> f32 {
    let x = left[0] - right[0];
    let y = left[1] - right[1];
    let z = left[2] - right[2];
    x * x + y * y + z * z
}

fn triangles_match(left: &[[f32; 3]; 3], right: &[[f32; 3]; 3]) -> bool {
    const PERMUTATIONS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    PERMUTATIONS.iter().any(|permutation| {
        permutation.iter().enumerate().all(|(index, other_index)| {
            let Some(left_point) = left.get(index) else {
                return false;
            };
            let Some(right_point) = right.get(*other_index) else {
                return false;
            };
            distance_squared(*left_point, *right_point) <= 0.0001_f32.powi(2) + 1.0e-10
        })
    })
}

fn directory_bytes(root: &Path) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_path_buf();
                files.push((relative, fs::read(path)?));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}
