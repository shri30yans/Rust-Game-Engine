//! Focused tests for the deterministic cooking helpers.
use std::error::Error;

use crate::mesh::{self, Vertex};
use crate::output::{self, stable_hash};
use crate::texture::accumulate_texture_sizes;
use engine_assets::format::IndexEncoding;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn pack_with_ids(ids: &[u64], source_id: Option<u64>) -> engine_assets::format::MeshPack {
    engine_assets::format::MeshPack {
        source_hash: 7,
        meshes: Vec::new(),
        materials: Vec::new(),
        nodes: ids
            .iter()
            .enumerate()
            .map(|(index, id)| engine_assets::format::Node {
                name: format!("object-{index}"),
                parent: None,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
                tags: std::iter::once(("asset_id".to_owned(), id.to_string()))
                    .chain(
                        source_id
                            .map(|source_id| ("source_asset_id".to_owned(), source_id.to_string())),
                    )
                    .collect(),
            })
            .collect(),
        clips: Vec::new(),
        collision: Vec::new(),
        textures: Vec::new(),
    }
}

fn temp_output(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cook-{name}-{}", std::process::id()))
}

#[test]
fn stable_index_resolves_ids_to_cooked_nodes_and_content_hashes() -> Result<(), Box<dyn Error>> {
    let directory = temp_output("index");
    fs::create_dir_all(&directory)?;
    let bytes = engine_assets::format::encode(
        &engine_assets::format::Asset::MeshPack(pack_with_ids(&[42], Some(99))),
        7,
    )?;
    fs::write(directory.join("arena.meshpack"), bytes)?;
    fs::write(directory.join("stale.meshpack"), b"stale source output")?;

    output::write_index(&directory, &["arena"])?;
    let index = fs::read_to_string(directory.join("index.ron"))?;
    assert!(index.contains("42"));
    assert!(index.contains("arena.meshpack"));
    assert!(index.contains("content_hash: 7"));
    assert!(index.contains("99"));
    assert!(index.contains("node: None"));
    assert!(index.contains("node: Some(0)"));
    assert!(!index.contains("stale.meshpack"));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stable_index_adds_prefab_files_and_rejects_missing_model_references()
-> Result<(), Box<dyn Error>> {
    let directory = temp_output("prefab-index");
    let prefab_dir = directory.join("prefabs");
    fs::create_dir_all(&prefab_dir)?;
    let bytes = engine_assets::format::encode(
        &engine_assets::format::Asset::MeshPack(pack_with_ids(&[42], None)),
        7,
    )?;
    fs::write(directory.join("arena.meshpack"), bytes)?;
    fs::write(
        prefab_dir.join("crate.prefab.ron"),
        "(asset_id: 77, model_id: 42, collision: None, surface: \"wood\", components: {})",
    )?;

    output::write_index(&directory, &["arena"])?;
    let index = fs::read_to_string(directory.join("index.ron"))?;
    assert!(index.contains("crate.prefab.ron"));
    assert!(index.contains("77"));

    fs::write(
        prefab_dir.join("broken.prefab.ron"),
        "(asset_id: 78, model_id: 999, collision: None, surface: \"wood\", components: {})",
    )?;
    let result = output::write_index(&directory, &["arena"]);
    assert!(matches!(
        result,
        Err(crate::CookError::MissingReference { asset_id: 999, .. })
    ));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stable_index_rejects_duplicate_object_ids() -> Result<(), Box<dyn Error>> {
    let directory = temp_output("duplicate-index");
    fs::create_dir_all(&directory)?;
    let bytes = engine_assets::format::encode(
        &engine_assets::format::Asset::MeshPack(pack_with_ids(&[42, 42], None)),
        7,
    )?;
    fs::write(directory.join("arena.meshpack"), bytes)?;

    let result = output::write_index(&directory, &["arena"]);
    assert!(matches!(
        result,
        Err(crate::CookError::DuplicateAssetId { asset_id: 42, .. })
    ));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stable_index_rejects_duplicate_source_file_ids() -> Result<(), Box<dyn Error>> {
    let directory = temp_output("duplicate-source-index");
    fs::create_dir_all(&directory)?;
    for (name, object_id) in [("first", 42), ("second", 43)] {
        let bytes = engine_assets::format::encode(
            &engine_assets::format::Asset::MeshPack(pack_with_ids(&[object_id], Some(99))),
            7,
        )?;
        fs::write(directory.join(format!("{name}.meshpack")), bytes)?;
    }

    let result = output::write_index(&directory, &["first", "second"]);
    assert!(matches!(
        result,
        Err(crate::CookError::DuplicateAssetId { asset_id: 99, .. })
    ));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn tangent_generation_produces_unit_tangent_with_handedness() -> Result<(), Box<dyn Error>> {
    let mut vertices = vec![
        Vertex {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
            ..Vertex::default()
        },
        Vertex {
            position: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
            ..Vertex::default()
        },
        Vertex {
            position: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
            ..Vertex::default()
        },
    ];
    mesh::generate_tangents(&mut vertices, &[0, 1, 2])?;
    for vertex in vertices {
        let length = vertex
            .tangent
            .iter()
            .take(3)
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((length - 1.0).abs() < 0.001);
        assert!(
            vertex
                .tangent
                .get(3)
                .is_some_and(|sign| *sign == 1.0 || *sign == -1.0)
        );
    }
    Ok(())
}

#[test]
fn vertex_stream_encodes_positions_attributes_and_indices_separately() -> Result<(), Box<dyn Error>>
{
    let vertices = vec![
        Vertex {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            ..Vertex::default()
        },
        Vertex {
            position: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0, -1.0],
            ..Vertex::default()
        },
        Vertex {
            position: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            ..Vertex::default()
        },
    ];
    let bounds = engine_assets::format::Bounds {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 0.0],
        center: [0.5, 0.5, 0.0],
        radius: 1.0,
    };
    let (lod, _) = mesh::encode_lod(&vertices, &[0, 1, 2], 1.0, 0.0, bounds)?;
    assert_eq!(lod.attributes.len(), 36);
    assert_eq!(lod.index_encoding, IndexEncoding::U16);
    assert_eq!(lod.index_count, 3);
    Ok(())
}

#[test]
fn rust_encoder_builds_uastc_mipmaps() -> Result<(), Box<dyn Error>> {
    let pixels = vec![128_u8; 8 * 8 * 4];
    let ktx2 = texture_encode::encode_ktx2(&pixels, 8, 8, true, false)?;
    let texture = engine_assets::TextureData { ktx2: ktx2.clone() };
    assert!(
        texture
            .transcode(engine_assets::GpuTextureFormatFamily::Rgba8)?
            .len()
            > 1
    );
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/encoder-mips.ktx2");
    fs::write(fixture, ktx2)?;
    Ok(())
}

#[test]
fn deterministic_hash_and_texture_estimate_are_stable() {
    assert_eq!(stable_hash(b"same"), stable_hash(b"same"));
    let mut totals = BTreeMap::new();
    accumulate_texture_sizes(&mut totals, 4, 4);
    assert_eq!(totals.get("Bc7"), Some(&48));
    assert_eq!(totals.get("Rgba8"), Some(&84));
}
