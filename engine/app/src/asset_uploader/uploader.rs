//! The uploader's frame step: applies store events, prepares textures, uploads each pack under the
//! frame's byte budget, and releases packs that were unloaded.
//! Invariant: a pack is Ready only after every part is uploaded; a reload swaps it in one step.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender};

use engine_assets::format::MeshPack;
use engine_assets::{AssetStore, ColourSpace, TextureData};
use jobs::JobPool;
use render::Gfx;

use crate::error::AppError;

use super::build::{PackBuild, TexturePart, destroy_build, release_build, take_texture_resources};
use super::mesh::{create_materials, pack_in_use, remove_gpu_pack};
use super::queue::upload_within_budget;
use super::state::{AssetKey, PackState, log_failure_once};
use super::texture::{PreparedResult, texture_colour_spaces};
use super::types::{UploadError, UploadStatus};

const PREPARED_TEXTURE_CHANNEL_CAPACITY: usize = 64;

/// Owns render resources created for loaded mesh packs.
pub struct AssetUploader {
    pub(super) packs: HashMap<AssetKey, PackState>,
    pub(super) order: Vec<AssetKey>,
    pub(super) prepared_sender: SyncSender<PreparedResult>,
    pub(super) prepared_receiver: Receiver<PreparedResult>,
}

impl AssetUploader {
    /// Creates an uploader with a bounded worker-result channel.
    #[must_use]
    pub fn new() -> Self {
        let (prepared_sender, prepared_receiver) =
            mpsc::sync_channel(PREPARED_TEXTURE_CHANNEL_CAPACITY);
        Self {
            packs: HashMap::new(),
            order: Vec::new(),
            prepared_sender,
            prepared_receiver,
        }
    }

    /// Returns the current GPU readiness for a store-owned mesh pack handle.
    #[must_use]
    pub fn status(&self, handle: &engine_assets::Handle<MeshPack>) -> UploadStatus<'_> {
        let key = AssetKey {
            slot: handle.slot(),
            generation: handle.generation(),
        };
        let Some(state) = self.packs.get(&key) else {
            return UploadStatus::Pending;
        };
        if let Some(pack) = state.current.as_ref() {
            return UploadStatus::Ready(pack);
        }
        if let Some(error) = state.failed.as_ref() {
            return UploadStatus::Failed(error);
        }
        UploadStatus::Pending
    }

    /// Returns whether the exact cooked revision behind `handle` is ready for drawing.
    #[must_use]
    pub fn is_ready(&self, handle: &engine_assets::Handle<MeshPack>, source_hash: u64) -> bool {
        matches!(self.status(handle), UploadStatus::Ready(pack) if pack.source_hash == source_hash)
    }

    /// Applies store events, prepares worker textures, and uploads FIFO parts under `byte_budget`.
    pub fn update(
        &mut self,
        store: &mut AssetStore,
        gfx: &mut Gfx,
        pool: &JobPool,
        byte_budget: u64,
    ) -> Result<u64, AppError> {
        let events = store.events().to_vec();
        self.apply_events(store, gfx, pool, events)?;
        store.clear_events();
        self.schedule_preparation(store, gfx, pool);
        self.collect_prepared();
        let mut uploaded = 0u64;
        for key in &self.order {
            let Some(state) = self.packs.get_mut(key) else {
                continue;
            };
            if state.failed.is_some() {
                log_failure_once(state);
                continue;
            }
            let build = state.staging.as_mut().or(state.current_build.as_mut());
            let Some(build) = build else {
                continue;
            };
            upload_within_budget(build, gfx, &mut uploaded, byte_budget)?;
            if build.next_part == build.textures.len() + build.meshes.len() {
                create_materials(build, gfx)?;
                if build.is_complete() {
                    let Some(new_pack) = build.gpu_pack() else {
                        continue;
                    };
                    if state.current.is_some() {
                        if let Some(old) = state.current.as_ref()
                            && (old.meshes.len() != new_pack.meshes.len()
                                || old.materials.len() != new_pack.materials.len())
                        {
                            state.failed = Some(UploadError::CountMismatch {
                                old_meshes: old.meshes.len(),
                                new_meshes: new_pack.meshes.len(),
                                old_materials: old.materials.len(),
                                new_materials: new_pack.materials.len(),
                            });
                            destroy_build(build, gfx);
                            for texture in &mut build.textures {
                                if let Some(handle) = texture.handle.take() {
                                    store.release(handle)?;
                                }
                            }
                            continue;
                        }
                        let (new_texture_handles, new_texture_ids) = take_texture_resources(build);
                        if let Some(old) = state.current.take() {
                            for (old_id, new_id) in old.meshes.iter().zip(new_pack.meshes.iter()) {
                                gfx.replace_mesh(*old_id, *new_id)?;
                            }
                            for (old_id, new_id) in
                                old.materials.iter().zip(new_pack.materials.iter())
                            {
                                gfx.replace_material(*old_id, *new_id)?;
                            }
                            remove_gpu_pack(gfx, old, &state.current_gpu_textures)?;
                            for handle in state.current_textures.drain(..) {
                                store.release(handle)?;
                            }
                        }
                        state.current = Some(new_pack);
                        state.current_textures = new_texture_handles;
                        state.current_gpu_textures = new_texture_ids;
                        state.staging = None;
                        state.failed = None;
                        state.logged_failure = None;
                    } else {
                        let (new_texture_handles, new_texture_ids) = take_texture_resources(build);
                        state.current = Some(new_pack);
                        state.current_textures = new_texture_handles;
                        state.current_gpu_textures = new_texture_ids;
                        state.current_build = None;
                        state.failed = None;
                        state.logged_failure = None;
                    }
                }
            }
        }
        self.remove_unloaded(store, gfx)?;
        Ok(uploaded)
    }

    fn apply_events(
        &mut self,
        store: &mut AssetStore,
        gfx: &mut Gfx,
        pool: &JobPool,
        events: Vec<engine_assets::StoreEvent>,
    ) -> Result<(), AppError> {
        for event in &events {
            let (id, reload, unload) = match *event {
                engine_assets::StoreEvent::Ready(id) if id.kind == TypeId::of::<MeshPack>() => {
                    (id, false, false)
                }
                engine_assets::StoreEvent::Reloaded(id) if id.kind == TypeId::of::<MeshPack>() => {
                    (id, true, false)
                }
                engine_assets::StoreEvent::Unloaded(id) if id.kind == TypeId::of::<MeshPack>() => {
                    (id, false, true)
                }
                _ => continue,
            };
            let key = AssetKey::from(id);
            if unload {
                if let Some(state) = self.packs.get_mut(&key) {
                    state.unload_queued = true;
                }
                continue;
            }
            let pack = store
                .current_by_id::<MeshPack>(id)?
                .cloned()
                .ok_or(AppError::ClientNotReady)?;
            let mut build = PackBuild::new(&pack);
            let texture_space = texture_colour_spaces(&pack);
            for hash in &pack.textures {
                let texture_path = engine_assets::AssetPath::new(format!(
                    "content/cooked/textures/{hash:016x}.ktx2"
                ))
                .map_err(|error| AppError::CookedAsset(error.to_string()))?;
                let handle = match store.load::<TextureData>(pool, texture_path) {
                    Ok(handle) => Some(handle),
                    Err(error) => {
                        build.failed = Some(UploadError::Texture {
                            texture: format!("{hash:016x}"),
                            reason: error.to_string(),
                        });
                        None
                    }
                };
                build.textures.push(TexturePart {
                    hash: *hash,
                    handle,
                    colour_space: texture_space
                        .get(hash)
                        .copied()
                        .unwrap_or(ColourSpace::Linear),
                    preparing: false,
                    prepared: None,
                    gpu: None,
                });
            }
            let is_new = !self.packs.contains_key(&key);
            let state = self.packs.entry(key).or_insert(PackState {
                current: None,
                current_textures: Vec::new(),
                current_gpu_textures: Vec::new(),
                current_build: None,
                staging: None,
                failed: None,
                unload_queued: false,
                logged_failure: None,
                unload_wait_logged: false,
            });
            if is_new {
                self.order.push(key);
            }
            if let Some(previous) = state.staging.take() {
                release_build(previous, store, gfx)?;
            }
            if let Some(previous) = state.current_build.take() {
                release_build(previous, store, gfx)?;
            }
            state.failed = None;
            if let Some(error) = build.failed.clone() {
                state.failed = Some(error);
            }
            if reload || state.current.is_some() {
                state.staging = Some(build);
            } else {
                state.current_build = Some(build);
            }
        }
        Ok(())
    }

    fn remove_unloaded(&mut self, store: &mut AssetStore, gfx: &mut Gfx) -> Result<(), AppError> {
        for state in self.packs.values_mut() {
            if !state.unload_queued {
                continue;
            }
            if state
                .current
                .as_ref()
                .is_some_and(|pack| pack_in_use(gfx, pack))
            {
                if !state.unload_wait_logged && cfg!(debug_assertions) {
                    platform::log_error("asset unload is waiting for live render primitives");
                    state.unload_wait_logged = true;
                }
                continue;
            }
            if let Some(pack) = state.current.take() {
                remove_gpu_pack(gfx, pack, &state.current_gpu_textures)?;
            }
            state.current_gpu_textures.clear();
            for handle in state.current_textures.drain(..) {
                store.release(handle)?;
            }
            for build in [&mut state.staging, &mut state.current_build]
                .into_iter()
                .filter_map(Option::take)
            {
                release_build(build, store, gfx)?;
            }
            state.unload_queued = false;
            state.failed = None;
            state.logged_failure = None;
            state.unload_wait_logged = false;
        }
        Ok(())
    }
}

impl Default for AssetUploader {
    fn default() -> Self {
        Self::new()
    }
}
