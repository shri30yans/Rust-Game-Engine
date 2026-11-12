//! Physically based material data and validation.
//!
//! Texture references must identify content hashes declared by the owning mesh pack.
use serde::{Deserialize, Serialize};

use super::MeshPack;

/// Physically based metallic/roughness material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Base colour factor in linear space.
    pub base_color: [f32; 4],
    /// Metallic factor.
    pub metallic: f32,
    /// Roughness factor.
    pub roughness: f32,
    /// Optional base colour texture index.
    pub base_color_texture: Option<u64>,
    /// Optional occlusion/roughness/metallic texture index.
    pub orm_texture: Option<u64>,
    /// Optional tangent-space normal texture index.
    pub normal_texture: Option<u64>,
    /// Optional emissive texture index.
    pub emissive_texture: Option<u64>,
    /// Marks a flat emissive overlay material.
    pub emissive: bool,
}

pub(super) fn is_valid(material: &Material, pack: &MeshPack) -> bool {
    let valid_texture =
        |texture: Option<u64>| texture.is_none_or(|hash| pack.textures.contains(&hash));
    material.base_color.iter().all(|value| value.is_finite())
        && material.metallic.is_finite()
        && material.roughness.is_finite()
        && valid_texture(material.base_color_texture)
        && valid_texture(material.orm_texture)
        && valid_texture(material.normal_texture)
        && valid_texture(material.emissive_texture)
}
