//! The presenter's view of the client's assets: the store, the uploader and the job pool.
//! Presenters get only this, never the uploader's internals.

use engine_assets::{AssetKind, AssetPath, AssetStore};
use jobs::JobPool;

use super::uploader::AssetUploader;

/// The presenter's only access to engine-owned cooked assets.
pub struct ClientAssets<'a> {
    /// Typed logical-path asset store owned by the client app.
    pub store: &'a mut AssetStore,
    /// GPU resources created for loaded mesh packs.
    pub uploader: &'a AssetUploader,
    pub(crate) pool: &'a JobPool,
}

impl ClientAssets<'_> {
    /// Loads or retains an asset by its validated content-pack path.
    pub fn load<T: AssetKind>(
        &mut self,
        path: AssetPath,
    ) -> Result<engine_assets::Handle<T>, engine_assets::store::StoreError> {
        self.store.load(self.pool, path)
    }

    /// Reads UTF-8 content from the mounted game pack.
    pub fn content_text(&self, path: &AssetPath) -> Result<&str, engine_assets::store::StoreError> {
        self.store.content_text(path)
    }

    /// Reads binary content from the mounted game pack.
    pub fn content_bytes(
        &self,
        path: &AssetPath,
    ) -> Result<&[u8], engine_assets::store::StoreError> {
        self.store.content_bytes(path)
    }

    /// Returns the current hash for a mounted content entry.
    pub fn content_hash(&self, path: &AssetPath) -> Result<u64, engine_assets::store::StoreError> {
        self.store.content_hash(path)
    }

    /// Returns the currently mounted pack for systems that parse structured game data.
    #[must_use]
    pub fn content_pack(&self) -> &engine_assets::ContentPack {
        self.store.content_pack()
    }

    /// Returns the current typed value behind a store handle.
    pub fn current<T: AssetKind>(
        &self,
        handle: &engine_assets::Handle<T>,
    ) -> Result<Option<&T>, engine_assets::store::StoreError> {
        self.store.current(handle)
    }

    /// Returns the stable logical path behind a store handle.
    pub fn path<T: AssetKind>(
        &self,
        handle: &engine_assets::Handle<T>,
    ) -> Result<&AssetPath, engine_assets::store::StoreError> {
        self.store.path_by_id::<T>(engine_assets::store::AssetId {
            kind: std::any::TypeId::of::<T>(),
            slot: handle.slot(),
            generation: handle.generation(),
        })
    }
}
