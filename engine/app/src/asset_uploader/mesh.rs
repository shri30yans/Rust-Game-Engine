//! Mesh and material upload: a pack's cooked meshes and materials go to the GPU, and a pack's GPU
//! resources are removed when it is released. A material's texture is looked up by content hash
//! among the build's uploaded textures; a missing one binds nothing.

use std::collections::HashMap;

use engine_assets::format::Material as PackMaterial;
use render::{Gfx, MaterialDesc, RenderError, TextureId};

use crate::error::AppError;

use super::build::PackBuild;
use super::types::GpuPack;

/// Uploads one cooked mesh; `part` is its index past the pack's textures. Returns its bytes, or
/// `None` when the pack has no such mesh.
pub(super) fn upload_mesh(
    build: &mut PackBuild,
    gfx: &mut Gfx,
    part: usize,
) -> Result<Option<u64>, AppError> {
    let index = part.saturating_sub(build.textures.len());
    let Some(mesh) = build.pack.meshes.get(index) else {
        return Ok(None);
    };
    let byte_count = build.upload_bytes(part).unwrap_or(0);
    let mesh_id = gfx.add_cooked_mesh(&mesh.lods)?;
    if let Some(place) = build.meshes.get_mut(index) {
        *place = Some(mesh_id);
    }
    Ok(Some(byte_count))
}

pub(super) fn create_materials(build: &mut PackBuild, gfx: &mut Gfx) -> Result<(), AppError> {
    for (index, material) in build.pack.materials.iter().enumerate() {
        if build.materials.get(index).is_some_and(Option::is_some) {
            continue;
        }
        let desc = material_desc(material, &build.texture_ids);
        let id = gfx.add_material(desc)?;
        if let Some(place) = build.materials.get_mut(index) {
            *place = Some(id);
        }
    }
    Ok(())
}

fn material_desc(material: &PackMaterial, textures: &HashMap<u64, TextureId>) -> MaterialDesc {
    MaterialDesc {
        base_color: material.base_color,
        metallic: material.metallic,
        roughness: material.roughness,
        emissive_strength: f32::from(material.emissive),
        normal_scale: 1.0,
        base_color_texture: material
            .base_color_texture
            .and_then(|hash| textures.get(&hash).copied()),
        orm_texture: material
            .orm_texture
            .and_then(|hash| textures.get(&hash).copied()),
        normal_texture: material
            .normal_texture
            .and_then(|hash| textures.get(&hash).copied()),
        emissive_texture: material
            .emissive_texture
            .and_then(|hash| textures.get(&hash).copied()),
        ..MaterialDesc::default()
    }
}

pub(super) fn pack_in_use(gfx: &Gfx, pack: &GpuPack) -> bool {
    gfx.pack_resources_in_use(&pack.meshes, &pack.materials)
}

pub(super) fn remove_gpu_pack(
    gfx: &mut Gfx,
    pack: GpuPack,
    textures: &[TextureId],
) -> Result<(), RenderError> {
    for material in pack.materials {
        gfx.remove_material(material)?;
    }
    for mesh in pack.meshes {
        gfx.remove_mesh(mesh)?;
    }
    for texture in textures {
        gfx.remove_texture(*texture)?;
    }
    Ok(())
}
