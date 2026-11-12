//! Collision selection, decomposition quality, and byte determinism from generated GLB fixtures.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use cook::cook_directory;
use engine_assets::format::{self, Asset, CollisionShape, MeshPack};
use engine_physics::{ConvexShape, DecompositionParams, decompose_mesh};
use glam::Vec3;
use serde_json::json;

#[derive(Clone, Copy)]
struct FixtureObject {
    name: &'static str,
    mesh: usize,
    role: Option<&'static str>,
    child_collision: Option<Option<&'static str>>,
}

const OBJECTS: [FixtureObject; 13] = [
    FixtureObject {
        name: "TaggedBox",
        mesh: 0,
        role: None,
        child_collision: Some(Some("box")),
    },
    FixtureObject {
        name: "TaggedHull",
        mesh: 0,
        role: None,
        child_collision: Some(Some("hull")),
    },
    FixtureObject {
        name: "TaggedMesh",
        mesh: 0,
        role: None,
        child_collision: Some(Some("mesh")),
    },
    FixtureObject {
        name: "TaggedDecompose",
        mesh: 0,
        role: None,
        child_collision: Some(Some("decompose")),
    },
    FixtureObject {
        name: "TaggedDefault",
        mesh: 0,
        role: None,
        child_collision: Some(None),
    },
    FixtureObject {
        name: "Static",
        mesh: 0,
        role: Some("static"),
        child_collision: None,
    },
    FixtureObject {
        name: "Prop",
        mesh: 0,
        role: Some("prop"),
        child_collision: None,
    },
    FixtureObject {
        name: "Mover",
        mesh: 0,
        role: Some("mover"),
        child_collision: None,
    },
    FixtureObject {
        name: "Breakable",
        mesh: 0,
        role: Some("breakable"),
        child_collision: None,
    },
    FixtureObject {
        name: "ConcaveProp",
        mesh: 1,
        role: Some("prop"),
        child_collision: None,
    },
    FixtureObject {
        name: "Trigger",
        mesh: 0,
        role: Some("trigger"),
        child_collision: None,
    },
    FixtureObject {
        name: "Unroled",
        mesh: 0,
        role: None,
        child_collision: None,
    },
    FixtureObject {
        name: "Override",
        mesh: 0,
        role: Some("prop"),
        child_collision: Some(Some("mesh")),
    },
];

#[test]
fn generated_glb_exercises_every_collision_selection_rule() -> Result<(), Box<dyn Error>> {
    let root = fixture_root("selection");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input)?;
    fs::write(input.join("selection.glb"), glb_fixture()?)?;

    cook_directory(&input, &output)?;
    let pack = read_pack(&output.join("selection.meshpack"))?;

    assert_shape(&pack, "TaggedBox", |shape| {
        matches!(shape, CollisionShape::Box { .. })
    })?;
    assert_shape(&pack, "TaggedHull", |shape| {
        matches!(shape, CollisionShape::Hull { .. })
    })?;
    assert_shape(&pack, "TaggedMesh", |shape| {
        matches!(shape, CollisionShape::TriMesh { .. })
    })?;
    assert_shape(&pack, "TaggedDecompose", |shape| {
        matches!(shape, CollisionShape::Decomposed { .. })
    })?;
    assert_shape(&pack, "TaggedDefault", |shape| {
        matches!(shape, CollisionShape::Hull { .. })
    })?;
    assert_shape(&pack, "Static", |shape| {
        matches!(shape, CollisionShape::TriMesh { .. })
    })?;
    assert_shape(&pack, "Prop", |shape| {
        matches!(shape, CollisionShape::Hull { .. })
    })?;
    assert_shape(&pack, "Mover", |shape| {
        matches!(shape, CollisionShape::Hull { .. })
    })?;
    assert_shape(&pack, "Breakable", |shape| {
        matches!(shape, CollisionShape::Hull { .. })
    })?;
    assert_shape(&pack, "ConcaveProp", |shape| {
        matches!(shape, CollisionShape::Decomposed { .. })
    })?;
    assert_shape(&pack, "Trigger", |shape| {
        matches!(shape, CollisionShape::Box { .. })
    })?;
    assert_shape(&pack, "Override", |shape| {
        matches!(shape, CollisionShape::TriMesh { .. })
    })?;
    assert!(collision_set(&pack, "Trigger")?.is_sensor);
    assert!(!pack.collision.iter().any(|set| {
        pack.nodes
            .get(usize::try_from(set.node).unwrap_or(usize::MAX))
            .is_some_and(|node| node.name == "Unroled")
    }));
    let report = fs::read_to_string(output.join("selection.report.ron"))?;
    assert!(report.contains("Unroled"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn l_solid_decomposes_into_multiple_hulls_with_bounded_volume_error() -> Result<(), Box<dyn Error>>
{
    let (vertices, indices) = l_prism();
    let hulls = decompose_mesh(&vertices, &indices, DecompositionParams::default())?;
    assert!(
        hulls.len() >= 2,
        "L-shaped solid needs multiple convex hulls"
    );
    let hull_volume = hulls.iter().try_fold(0.0, |volume, points| {
        Ok::<f32, Box<dyn Error>>(volume + ConvexShape::hull(points)?.volume())
    })?;
    let solid_volume = 3.0_f32;
    let relative_error = (hull_volume - solid_volume).abs() / solid_volume;
    assert!(
        relative_error <= 0.1,
        "hull volume {hull_volume} differs from solid volume by {relative_error:.1}%"
    );
    Ok(())
}

#[test]
fn collision_cooking_produces_identical_pack_and_report_bytes() -> Result<(), Box<dyn Error>> {
    let root = fixture_root("determinism");
    let input = root.join("input");
    let first = root.join("first");
    let second = root.join("second");
    fs::create_dir_all(&input)?;
    fs::write(input.join("selection.glb"), glb_fixture()?)?;

    cook_directory(&input, &first)?;
    cook_directory(&input, &second)?;
    assert_eq!(directory_bytes(&first)?, directory_bytes(&second)?);
    fs::remove_dir_all(root)?;
    Ok(())
}

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/cook-collision-fixtures")
        .join(name)
}

type OutputFiles = Vec<(PathBuf, Vec<u8>)>;

fn directory_bytes(directory: &Path) -> Result<OutputFiles, Box<dyn Error>> {
    let mut files: OutputFiles = fs::read_dir(directory)?
        .map(|entry| {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file() {
                let name = path
                    .file_name()
                    .map(PathBuf::from)
                    .ok_or_else(|| std::io::Error::other("output file has no name"))?;
                Ok(Some((name, fs::read(path)?)))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?
        .into_iter()
        .flatten()
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn read_pack(path: &Path) -> Result<MeshPack, Box<dyn Error>> {
    let (_, Asset::MeshPack(pack)) = format::decode(&fs::read(path)?)?;
    Ok(pack)
}

fn collision_set<'a>(
    pack: &'a MeshPack,
    name: &str,
) -> Result<&'a engine_assets::format::CollisionSet, Box<dyn Error>> {
    pack.collision
        .iter()
        .find(|set| {
            pack.nodes
                .get(usize::try_from(set.node).unwrap_or(usize::MAX))
                .is_some_and(|node| node.name == name)
        })
        .ok_or_else(|| format!("collision set for {name} is missing").into())
}

fn assert_shape(
    pack: &MeshPack,
    name: &str,
    predicate: impl FnOnce(&CollisionShape) -> bool,
) -> Result<(), Box<dyn Error>> {
    let set = collision_set(pack, name)?;
    let shape = set
        .shapes
        .first()
        .ok_or_else(|| format!("collision set for {name} has no shapes"))?;
    assert!(
        predicate(shape),
        "wrong collision kind for {name}: {shape:?}"
    );
    Ok(())
}

fn glb_fixture() -> Result<Vec<u8>, Box<dyn Error>> {
    let (tetra_vertices, tetra_faces) = tetrahedron();
    let (l_vertices, l_faces) = l_prism();
    let mut binary = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    for (vertices, faces) in [(&tetra_vertices, &tetra_faces), (&l_vertices, &l_faces)] {
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        let position_offset = binary.len();
        for point in vertices {
            for component in point.to_array() {
                binary.extend_from_slice(&component.to_le_bytes());
            }
        }
        let position_length = binary.len() - position_offset;
        let position_view = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0, "byteOffset": position_offset, "byteLength": position_length, "target": 34962
        }));
        let position_accessor = accessors.len();
        let (minimum, maximum) = bounds(vertices);
        accessors.push(json!({
            "bufferView": position_view, "componentType": 5126, "count": vertices.len(),
            "type": "VEC3", "min": minimum, "max": maximum
        }));

        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        let index_offset = binary.len();
        for triangle in faces {
            for index in triangle {
                binary.extend_from_slice(&u16::try_from(*index)?.to_le_bytes());
            }
        }
        let index_length = binary.len() - index_offset;
        let index_view = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0, "byteOffset": index_offset, "byteLength": index_length, "target": 34963
        }));
        let index_accessor = accessors.len();
        accessors.push(json!({
            "bufferView": index_view, "componentType": 5123, "count": faces.len() * 3, "type": "SCALAR"
        }));
        meshes.push(json!({
            "primitives": [{ "attributes": { "POSITION": position_accessor }, "indices": index_accessor }]
        }));
    }

    let mut nodes = Vec::new();
    let mut roots = Vec::new();
    for object in OBJECTS {
        let owner_index = nodes.len();
        roots.push(owner_index);
        let extras = if let Some(role) = object.role {
            Some(json!({"role": role, "material": "concrete"}))
        } else if object.name == "Unroled" {
            Some(json!({"material": "wood"}))
        } else {
            None
        };
        if let Some(collision) = object.child_collision {
            let child_index = owner_index
                .checked_add(1)
                .ok_or("fixture node index overflow")?;
            let owner = if let Some(extras) = extras {
                json!({ "name": object.name, "mesh": object.mesh, "extras": extras, "children": [child_index] })
            } else {
                json!({ "name": object.name, "mesh": object.mesh, "children": [child_index] })
            };
            nodes.push(owner);
            let child = if let Some(tag) = collision {
                json!({ "name": format!("{}_COL", object.name), "mesh": 0, "extras": {"collision": tag} })
            } else {
                json!({ "name": format!("{}_COL", object.name), "mesh": 0 })
            };
            nodes.push(child);
        } else {
            let owner = if let Some(extras) = extras {
                json!({ "name": object.name, "mesh": object.mesh, "extras": extras })
            } else {
                json!({ "name": object.name, "mesh": object.mesh })
            };
            nodes.push(owner);
        }
    }

    let json_value = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": roots}],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": binary.len()}]
    });
    let mut json_chunk = serde_json::to_vec(&json_value)?;
    while !json_chunk.len().is_multiple_of(4) {
        json_chunk.push(b' ');
    }
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let total_length = 12 + 8 + json_chunk.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(total_length);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2_u32.to_le_bytes());
    glb.extend_from_slice(&u32::try_from(total_length)?.to_le_bytes());
    glb.extend_from_slice(&u32::try_from(json_chunk.len())?.to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534A_u32.to_le_bytes());
    glb.extend_from_slice(&json_chunk);
    glb.extend_from_slice(&u32::try_from(binary.len())?.to_le_bytes());
    glb.extend_from_slice(&0x004E_4942_u32.to_le_bytes());
    glb.extend_from_slice(&binary);
    Ok(glb)
}

fn tetrahedron() -> (Vec<Vec3>, Vec<[u32; 3]>) {
    (
        vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z],
        vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    )
}

fn l_prism() -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let footprint = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(1.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    let vertices = footprint
        .into_iter()
        .chain(footprint.into_iter().map(|point| point + Vec3::Z))
        .collect::<Vec<_>>();
    let mut triangles = vec![
        [6, 7, 8],
        [6, 8, 9],
        [6, 9, 10],
        [6, 10, 11],
        [0, 2, 1],
        [0, 3, 2],
        [0, 4, 3],
        [0, 5, 4],
    ];
    for edge in 0_u32..6 {
        let next = (edge + 1) % 6;
        let top = edge + 6;
        let next_top = next + 6;
        triangles.push([edge, next, next_top]);
        triangles.push([edge, next_top, top]);
    }
    (vertices, triangles)
}

fn bounds(vertices: &[Vec3]) -> ([f32; 3], [f32; 3]) {
    let first = vertices.first().copied().unwrap_or(Vec3::ZERO);
    let mut minimum = first;
    let mut maximum = first;
    for point in vertices.iter().copied().skip(1) {
        minimum = minimum.min(point);
        maximum = maximum.max(point);
    }
    (minimum.to_array(), maximum.to_array())
}
