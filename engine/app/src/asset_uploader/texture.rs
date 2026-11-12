//! Texture upload: the store loads of a pack's textures, worker preparation of their mip data,
//! and the GPU upload of each prepared texture.

use std::collections::HashMap;

use engine_assets::format::{AssetError, MeshPack};
use engine_assets::{AssetStore, ColourSpace, PreparedTexture, prepare_texture};
use jobs::JobPool;
use render::Gfx;

use crate::error::AppError;

use super::build::PackBuild;
use super::state::AssetKey;
use super::types::UploadError;
use super::uploader::AssetUploader;

pub(super) struct PreparedResult {
    pub(super) asset: AssetKey,
    pub(super) hash: u64,
    pub(super) prepared: Result<PreparedTexture, AssetError>,
}

pub(super) fn texture_colour_spaces(pack: &MeshPack) -> HashMap<u64, ColourSpace> {
    let mut colours = HashMap::with_capacity(pack.textures.len());
    for material in &pack.materials {
        insert_space(&mut colours, material.base_color_texture, ColourSpace::Srgb);
        insert_space(&mut colours, material.emissive_texture, ColourSpace::Srgb);
        insert_space(&mut colours, material.normal_texture, ColourSpace::Linear);
        insert_space(&mut colours, material.orm_texture, ColourSpace::Linear);
    }
    colours
}

fn insert_space(spaces: &mut HashMap<u64, ColourSpace>, hash: Option<u64>, space: ColourSpace) {
    if let Some(hash) = hash {
        spaces.entry(hash).or_insert(space);
    }
}

/// Uploads one prepared texture and returns its bytes, or `None` while it is still being prepared.
pub(super) fn upload_texture(
    build: &mut PackBuild,
    gfx: &mut Gfx,
    part: usize,
) -> Result<Option<u64>, AppError> {
    let Some(texture) = build.textures.get_mut(part) else {
        return Ok(None);
    };
    let Some(prepared) = texture.prepared.take() else {
        return Ok(None);
    };
    let byte_count = prepared.mips.iter().fold(0u64, |total, mip| {
        total.saturating_add(u64::try_from(mip.bytes.len()).unwrap_or(u64::MAX))
    });
    match gfx.add_texture(prepared) {
        Ok(id) => {
            texture.gpu = Some(id);
            build.texture_ids.insert(texture.hash, id);
            Ok(Some(byte_count))
        }
        Err(error) => Err(UploadError::Render(error.to_string()).into()),
    }
}

impl AssetUploader {
    pub(super) fn schedule_preparation(&mut self, store: &AssetStore, gfx: &Gfx, pool: &JobPool) {
        let family = gfx.texture_family();
        for (key, state) in &mut self.packs {
            if state.failed.is_some() {
                continue;
            }
            let mut failure = None;
            for build in [state.staging.as_mut(), state.current_build.as_mut()]
                .into_iter()
                .flatten()
            {
                for texture in &mut build.textures {
                    if texture.preparing || texture.prepared.is_some() || texture.gpu.is_some() {
                        continue;
                    }
                    let Some(handle) = texture.handle.as_ref() else {
                        failure = Some(UploadError::Texture {
                            texture: format!("{:016x}", texture.hash),
                            reason: "no store handle was created".to_owned(),
                        });
                        continue;
                    };
                    let data = match store.status(handle) {
                        Ok(engine_assets::AssetStatus::Ready(data)) => data.clone(),
                        Ok(engine_assets::AssetStatus::Loading) => continue,
                        Ok(engine_assets::AssetStatus::Failed(error)) => {
                            failure = Some(UploadError::Texture {
                                texture: format!("{:016x}", texture.hash),
                                reason: error.to_string(),
                            });
                            continue;
                        }
                        Err(error) => {
                            failure = Some(UploadError::Texture {
                                texture: format!("{:016x}", texture.hash),
                                reason: error.to_string(),
                            });
                            continue;
                        }
                    };
                    let sender = self.prepared_sender.clone();
                    let asset = *key;
                    let hash = texture.hash;
                    let colour_space = texture.colour_space;
                    pool.spawn(move || {
                        let prepared = prepare_texture(&data, colour_space, family);
                        let _ = sender.send(PreparedResult {
                            asset,
                            hash,
                            prepared,
                        });
                    });
                    texture.preparing = true;
                }
            }
            if failure.is_some() {
                state.failed = failure;
            }
        }
    }

    pub(super) fn collect_prepared(&mut self) {
        while let Ok(result) = self.prepared_receiver.try_recv() {
            let Some(state) = self.packs.get_mut(&result.asset) else {
                continue;
            };
            let build = state.staging.as_mut().or(state.current_build.as_mut());
            let Some(build) = build else {
                continue;
            };
            let Some(texture) = build
                .textures
                .iter_mut()
                .find(|texture| texture.hash == result.hash)
            else {
                continue;
            };
            match result.prepared {
                Ok(prepared) => texture.prepared = Some(prepared),
                Err(error) => {
                    state.failed = Some(UploadError::Texture {
                        texture: format!("{:016x}", result.hash),
                        reason: error.to_string(),
                    });
                }
            }
        }
    }
}
