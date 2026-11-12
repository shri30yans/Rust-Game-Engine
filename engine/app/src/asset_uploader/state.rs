//! Per-handle bookkeeping for the uploader: the asset key, the live and in-flight state of each
//! pack, and the log line written once per failure.

use engine_assets::TextureData;
use engine_assets::store::AssetId;
use render::TextureId;

use super::build::PackBuild;
use super::types::{GpuPack, UploadError};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) struct AssetKey {
    pub(super) slot: u32,
    pub(super) generation: u32,
}

impl From<AssetId> for AssetKey {
    fn from(id: AssetId) -> Self {
        Self {
            slot: id.slot,
            generation: id.generation,
        }
    }
}

pub(super) struct PackState {
    pub(super) current: Option<GpuPack>,
    pub(super) current_textures: Vec<engine_assets::Handle<TextureData>>,
    pub(super) current_gpu_textures: Vec<TextureId>,
    pub(super) current_build: Option<PackBuild>,
    pub(super) staging: Option<PackBuild>,
    pub(super) failed: Option<UploadError>,
    pub(super) unload_queued: bool,
    pub(super) logged_failure: Option<UploadError>,
    pub(super) unload_wait_logged: bool,
}

pub(super) fn log_failure_once(state: &mut PackState) {
    let Some(error) = state.failed.as_ref() else {
        return;
    };
    if state.logged_failure.as_ref() == Some(error) {
        return;
    }
    platform::log_error(&format!("asset upload failed: {error}"));
    state.logged_failure = Some(error.clone());
}
