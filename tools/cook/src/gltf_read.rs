//! Reads glTF hierarchy and material metadata into engine-neutral cooked types.
//!
//! Parent-first ordering and sorted string tags preserve generic source data without game names.
use std::collections::BTreeMap;

use engine_assets::format::{Material, Node};
use gltf::Document;

use crate::CookError;

/// Lookup from source image and color-space role to the cooked texture hash.
pub(super) type TextureRefs = BTreeMap<(usize, bool), u64>;

/// Reads the active glTF scene in parent-first order and maps source node indices.
pub(super) fn read_nodes(
    document: &Document,
) -> Result<(Vec<Node>, BTreeMap<usize, u32>), CookError> {
    let mut nodes = Vec::new();
    let mut node_map = BTreeMap::new();
    if let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        for root in scene.nodes() {
            append_node(root, None, &mut nodes, &mut node_map)?;
        }
    }
    Ok((nodes, node_map))
}

/// Converts material factors and source texture slots to content-hash references.
pub(super) fn read_materials(document: &Document, texture_refs: &TextureRefs) -> Vec<Material> {
    let mut materials = Vec::new();
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        let map_texture = |index: Option<usize>, linear: bool| {
            index.and_then(|key| texture_refs.get(&(key, linear)).copied())
        };
        materials.push(Material {
            base_color: pbr.base_color_factor(),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            base_color_texture: map_texture(
                pbr.base_color_texture()
                    .map(|texture| texture.texture().source().index()),
                false,
            ),
            orm_texture: map_texture(
                pbr.metallic_roughness_texture()
                    .map(|texture| texture.texture().source().index()),
                true,
            ),
            normal_texture: map_texture(
                material
                    .normal_texture()
                    .map(|texture| texture.texture().source().index()),
                true,
            ),
            emissive_texture: map_texture(
                material
                    .emissive_texture()
                    .map(|texture| texture.texture().source().index()),
                false,
            ),
            emissive: material.emissive_factor().iter().any(|value| *value > 0.0)
                || material.emissive_texture().is_some(),
        });
    }
    if materials.is_empty() {
        materials.push(Material {
            base_color: [1.0; 4],
            metallic: 1.0,
            roughness: 1.0,
            base_color_texture: None,
            orm_texture: None,
            normal_texture: None,
            emissive_texture: None,
            emissive: false,
        });
    }
    materials
}

/// Preserves an explicitly authored clip loop flag from the extras object.
pub(super) fn parse_looping(extras: &gltf::json::extras::RawValue) -> Option<bool> {
    let values = serde_json::from_str::<serde_json::Value>(extras.get()).ok()?;
    values
        .get("loop")
        .or_else(|| values.get("looping"))
        .and_then(serde_json::Value::as_bool)
}

fn parse_tags(extras: &gltf::json::extras::RawValue) -> Vec<(String, String)> {
    match serde_json::from_str::<serde_json::Value>(extras.get()) {
        Ok(serde_json::Value::Object(values)) => values
            .into_iter()
            .map(|(key, value)| {
                let rendered = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                (key, rendered)
            })
            .collect(),
        Ok(value) => vec![("value".to_owned(), value.to_string())],
        Err(_) => vec![("raw".to_owned(), extras.get().to_owned())],
    }
}
fn append_node(
    node: gltf::Node<'_>,
    parent: Option<u32>,
    output: &mut Vec<Node>,
    map: &mut BTreeMap<usize, u32>,
) -> Result<(), CookError> {
    let index = u32::try_from(output.len()).map_err(|_| CookError::Mesh)?;
    map.insert(node.index(), index);
    let (translation, rotation, scale) = node.transform().decomposed();
    let tags = node.extras().as_deref().map(parse_tags).unwrap_or_default();
    output.push(Node {
        name: node.name().unwrap_or("").to_owned(),
        parent,
        translation,
        rotation,
        scale,
        tags,
    });
    for child in node.children() {
        append_node(child, Some(index), output, map)?;
    }
    Ok(())
}
