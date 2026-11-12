//! One in-flight pack build: the mesh, material and texture slots the uploader fills in order,
//! and the release of a build's GPU and store resources.

use engine_assets::format::MeshPack;
use engine_assets::{AssetStore, ColourSpace, PreparedTexture, TextureData};
use render::{Gfx, MaterialId, MeshId, TextureId};
use std::collections::HashMap;

use crate::error::AppError;

use super::types::{GpuPack, UploadError};

pub(super) struct TexturePart {
    pub(super) hash: u64,
    pub(super) handle: Option<engine_assets::Handle<TextureData>>,
    pub(super) colour_space: ColourSpace,
    pub(super) preparing: bool,
    pub(super) prepared: Option<PreparedTexture>,
    pub(super) gpu: Option<TextureId>,
}

pub(super) struct PackBuild {
    pub(super) meshes: Vec<Option<MeshId>>,
    pub(super) materials: Vec<Option<MaterialId>>,
    pub(super) textures: Vec<TexturePart>,
    pub(super) texture_ids: HashMap<u64, TextureId>,
    pub(super) next_part: usize,
    pub(super) pack: MeshPack,
    pub(super) failed: Option<UploadError>,
}

impl PackBuild {
    pub(super) fn new(pack: &MeshPack) -> Self {
        Self {
            meshes: vec![None; pack.meshes.len()],
            materials: vec![None; pack.materials.len()],
            textures: Vec::with_capacity(pack.textures.len()),
            texture_ids: HashMap::with_capacity(pack.textures.len()),
            next_part: 0,
            pack: pack.clone(),
            failed: None,
        }
    }

    pub(super) fn is_complete(&self) -> bool {
        self.next_part == self.textures.len() + self.meshes.len()
            && self.textures.iter().all(|texture| texture.gpu.is_some())
            && self.meshes.iter().all(Option::is_some)
            && self.materials.iter().all(Option::is_some)
    }

    pub(super) fn gpu_pack(&self) -> Option<GpuPack> {
        Some(GpuPack {
            source_hash: self.pack.source_hash,
            meshes: self.meshes.iter().copied().collect::<Option<Vec<_>>>()?,
            materials: self.materials.iter().copied().collect::<Option<Vec<_>>>()?,
        })
    }

    pub(super) fn upload_bytes(&self, part: usize) -> Option<u64> {
        if let Some(texture) = self.textures.get(part) {
            return texture.prepared.as_ref().map(|prepared| {
                prepared
                    .mips
                    .iter()
                    .map(|mip| u64::try_from(mip.bytes.len()).unwrap_or(u64::MAX))
                    .sum()
            });
        }
        let mesh_index = part.checked_sub(self.textures.len())?;
        self.pack
            .meshes
            .get(mesh_index)?
            .lods
            .iter()
            .try_fold(0u64, |bytes, lod| {
                let stream_bytes = u64::try_from(lod.positions.len())
                    .ok()?
                    .checked_add(u64::try_from(lod.attributes.len()).ok()?)?
                    .checked_add(u64::try_from(lod.indices.len()).ok()?)?;
                bytes.checked_add(stream_bytes)
            })
    }
}

pub(super) fn destroy_build(build: &mut PackBuild, gfx: &mut Gfx) {
    for material in build.materials.iter_mut().filter_map(Option::take) {
        let _ = gfx.remove_material(material);
    }
    for mesh in build.meshes.iter_mut().filter_map(Option::take) {
        let _ = gfx.remove_mesh(mesh);
    }
    for texture in &mut build.textures {
        if let Some(id) = texture.gpu.take() {
            let _ = gfx.remove_texture(id);
        }
    }
}

pub(super) fn take_texture_resources(
    build: &mut PackBuild,
) -> (Vec<engine_assets::Handle<TextureData>>, Vec<TextureId>) {
    let handles = build
        .textures
        .iter_mut()
        .filter_map(|texture| texture.handle.take())
        .collect();
    let ids = build
        .textures
        .iter_mut()
        .filter_map(|texture| texture.gpu.take())
        .collect();
    (handles, ids)
}

pub(super) fn release_build(
    mut build: PackBuild,
    store: &mut AssetStore,
    gfx: &mut Gfx,
) -> Result<(), AppError> {
    destroy_build(&mut build, gfx);
    for handle in build
        .textures
        .into_iter()
        .filter_map(|texture| texture.handle)
    {
        store.release(handle)?;
    }
    Ok(())
}
