//! One manager owns one generic typed slot store for every registered asset kind.
//!
//! Paths act as soft references. Handles keep successfully loaded values alive; file loading and
//! reloads run on the background pool, and only `apply_loaded` changes frame-visible values.
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use engine_core::{SlotId, Slots};
use jobs::JobPool;
use thiserror::Error;

use crate::format::{MeshPack, TextureData};
use crate::handle::Handle;
use crate::load::{self, LoadError};
use crate::{AssetPath, ContentPack, PackError};

/// An asset kind that can be loaded from a cooked soft path.
pub trait AssetKind: Send + Sync + 'static {
    /// Decodes this kind of asset from validated content bytes.
    fn load(bytes: &[u8]) -> Result<Self, LoadError>
    where
        Self: Sized;
}

impl AssetKind for MeshPack {
    fn load(bytes: &[u8]) -> Result<Self, LoadError> {
        load::load_mesh_pack(bytes)
    }
}

impl AssetKind for TextureData {
    fn load(bytes: &[u8]) -> Result<Self, LoadError> {
        load::load_texture(bytes)
    }
}

/// Typed load error reported for an asset slot.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct AssetFailure(#[from] LoadError);

impl AssetFailure {
    /// Returns the original typed file or format error.
    #[must_use]
    pub fn error(&self) -> &LoadError {
        &self.0
    }
}

/// Current state of a typed asset handle.
#[derive(Debug)]
pub enum AssetStatus<'a, T> {
    /// The background job has not completed.
    Loading,
    /// The asset is available.
    Ready(&'a T),
    /// Initial loading or reloading failed; a prior value remains available via `current`.
    Failed(&'a AssetFailure),
}

/// Opaque kind and generation identity of an asset freed by the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetId {
    /// Rust type identity of the registered asset kind.
    pub kind: TypeId,
    /// Store slot index.
    pub slot: u32,
    /// Generation that was removed.
    pub generation: u32,
}

/// Store events requiring renderer-side resource cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreEvent {
    /// A first load became visible at the frame boundary.
    Ready(AssetId),
    /// A successful hot reload replaced the current value at the frame boundary.
    Reloaded(AssetId),
    /// A released slot became available for reuse.
    Unloaded(AssetId),
}

/// Store operation failure.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Background worker failed to deliver its result.
    #[error("asset background task was lost")]
    TaskLost,
    /// Handle does not identify a live slot.
    #[error("asset handle is stale or invalid")]
    StaleHandle,
    /// Asset kind was not registered with this store.
    #[error("asset kind is not registered")]
    KindNotRegistered,
    /// Logical path is not present in the mounted game pack.
    #[error(transparent)]
    Pack(#[from] PackError),
}

type LoadResult<T> = Result<T, LoadError>;
struct Pending<T> {
    result: Receiver<LoadResult<T>>,
}
struct Slot<T> {
    refs: u32,
    path: AssetPath,
    source_hash: u64,
    value: Option<T>,
    failure: Option<AssetFailure>,
    pending: Option<Pending<T>>,
}

/// Typed slots for one registered asset kind.
pub struct Assets<T: AssetKind> {
    slots: Slots<Slot<T>>,
}

impl<T: AssetKind> Assets<T> {
    /// Creates an empty typed asset collection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Slots::new(),
        }
    }

    fn load(
        &mut self,
        pool: &JobPool,
        path: AssetPath,
        bytes: Vec<u8>,
        source_hash: u64,
    ) -> Result<Handle<T>, StoreError> {
        if let Some((id, slot)) = self.slots.iter_mut().find(|(_, slot)| slot.path == path) {
            slot.refs = slot.refs.checked_add(1).ok_or(StoreError::StaleHandle)?;
            return Ok(Handle::new(id.index(), id.generation()));
        }
        let slot = Slot {
            refs: 1,
            path,
            source_hash,
            value: None,
            failure: None,
            pending: Some(start_load::<T>(pool, bytes)),
        };
        let id = self.slots.insert(slot).ok_or(StoreError::StaleHandle)?;
        Ok(Handle::new(id.index(), id.generation()))
    }

    fn retain(&mut self, handle: &Handle<T>) -> Result<Handle<T>, StoreError> {
        let slot = self.slot_mut(handle)?;
        slot.refs = slot.refs.checked_add(1).ok_or(StoreError::StaleHandle)?;
        Ok(Handle::new(handle.slot, handle.generation))
    }

    fn release(&mut self, handle: Handle<T>) -> Result<(), StoreError> {
        let slot = self.slot_mut(&handle)?;
        slot.refs = slot.refs.checked_sub(1).ok_or(StoreError::StaleHandle)?;
        Ok(())
    }

    fn status(&self, handle: &Handle<T>) -> Result<AssetStatus<'_, T>, StoreError> {
        let slot = self.slot(handle)?;
        if let Some(failure) = slot.failure.as_ref() {
            return Ok(AssetStatus::Failed(failure));
        }
        if let Some(value) = slot.value.as_ref() {
            return Ok(AssetStatus::Ready(value));
        }
        Ok(AssetStatus::Loading)
    }

    fn current(&self, handle: &Handle<T>) -> Result<Option<&T>, StoreError> {
        Ok(self.slot(handle)?.value.as_ref())
    }

    fn reload_changed(&mut self, pool: &JobPool, content: &ContentPack) -> Result<(), StoreError> {
        for (_, slot) in self.slots.iter_mut() {
            if slot.refs == 0 || slot.pending.is_some() {
                continue;
            }
            match content.hash(&slot.path) {
                Ok(hash) if hash != slot.source_hash => {
                    let bytes = content.load(&slot.path)?.to_vec();
                    slot.source_hash = hash;
                    slot.pending = Some(start_load::<T>(pool, bytes));
                    slot.failure = None;
                }
                Err(error) => slot.failure = Some(AssetFailure(LoadError::Pack(error))),
                _ => {}
            }
        }
        Ok(())
    }

    fn path(&self, handle: &Handle<T>) -> Result<&AssetPath, StoreError> {
        Ok(&self.slot(handle)?.path)
    }

    fn apply_loaded(
        &mut self,
        kind: TypeId,
        _now: Instant,
        events: &mut Vec<StoreEvent>,
    ) -> Result<(), StoreError> {
        let mut unloaded = Vec::new();
        for (slot_id, slot) in self.slots.iter_mut() {
            if slot.refs == 0 {
                events.push(StoreEvent::Unloaded(AssetId {
                    kind,
                    slot: slot_id.index(),
                    generation: slot_id.generation(),
                }));
                unloaded.push(slot_id);
                continue;
            }
            let Some(pending) = slot.pending.as_mut() else {
                continue;
            };
            match pending.result.try_recv() {
                Ok(Ok(value)) => {
                    let id = AssetId {
                        kind,
                        slot: slot_id.index(),
                        generation: slot_id.generation(),
                    };
                    events.push(if slot.value.is_some() {
                        StoreEvent::Reloaded(id)
                    } else {
                        StoreEvent::Ready(id)
                    });
                    slot.value = Some(value);
                    slot.failure = None;
                    slot.pending = None;
                }
                Ok(Err(error)) => {
                    slot.failure = Some(AssetFailure(error));
                    slot.pending = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Err(StoreError::TaskLost),
            }
        }
        for slot in unloaded {
            let _ = self.slots.remove(slot);
        }
        Ok(())
    }

    fn slot(&self, handle: &Handle<T>) -> Result<&Slot<T>, StoreError> {
        self.slots
            .get(SlotId::new(handle.slot, handle.generation))
            .filter(|slot| slot.refs > 0)
            .ok_or(StoreError::StaleHandle)
    }

    fn slot_mut(&mut self, handle: &Handle<T>) -> Result<&mut Slot<T>, StoreError> {
        self.slots
            .get_mut(SlotId::new(handle.slot, handle.generation))
            .filter(|slot| slot.refs > 0)
            .ok_or(StoreError::StaleHandle)
    }
}

fn start_load<T: AssetKind>(pool: &JobPool, bytes: Vec<u8>) -> Pending<T> {
    let (sender, result) = mpsc::sync_channel(1);
    pool.spawn(move || {
        let loaded = T::load(&bytes);
        let _ = sender.send(loaded);
    });
    Pending { result }
}

impl<T: AssetKind> Default for Assets<T> {
    fn default() -> Self {
        Self::new()
    }
}

trait ErasedAssets: Any {
    fn as_any(&self) -> &dyn Any;
    fn apply(
        &mut self,
        type_id: TypeId,
        now: Instant,
        events: &mut Vec<StoreEvent>,
    ) -> Result<(), StoreError>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn reload_changed(&mut self, pool: &JobPool, content: &ContentPack) -> Result<(), StoreError>;
}

impl<T: AssetKind> ErasedAssets for Assets<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn apply(
        &mut self,
        type_id: TypeId,
        now: Instant,
        events: &mut Vec<StoreEvent>,
    ) -> Result<(), StoreError> {
        self.apply_loaded(type_id, now, events)
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn reload_changed(&mut self, pool: &JobPool, content: &ContentPack) -> Result<(), StoreError> {
        Assets::<T>::reload_changed(self, pool, content)
    }
}

/// Explicit owner of registered typed asset collections.
pub struct AssetStore {
    kinds: HashMap<TypeId, Box<dyn ErasedAssets>>,
    order: Vec<TypeId>,
    events: Vec<StoreEvent>,
    content: ContentPack,
}

impl AssetStore {
    /// Creates a manager and registers mesh packs and textures.
    #[must_use]
    pub fn new() -> Self {
        let mut store = Self {
            kinds: HashMap::new(),
            order: Vec::new(),
            events: Vec::new(),
            content: ContentPack::default(),
        };
        store.register::<MeshPack>();
        store.register::<TextureData>();
        store
    }

    /// Registers a new asset kind. Re-registering an existing kind leaves its slots intact.
    pub fn register<T: AssetKind>(&mut self) {
        let id = TypeId::of::<T>();
        if self.kinds.contains_key(&id) {
            return;
        }
        self.kinds.insert(id, Box::new(Assets::<T>::new()));
        self.order.push(id);
    }

    /// Mounts the validated game's content pack before any asset handles are requested.
    pub fn mount_content(
        &mut self,
        content: ContentPack,
        pool: &JobPool,
    ) -> Result<(), StoreError> {
        self.content = content;
        for type_id in &self.order {
            let Some(assets) = self.kinds.get_mut(type_id) else {
                return Err(StoreError::KindNotRegistered);
            };
            assets.reload_changed(pool, &self.content)?;
        }
        Ok(())
    }

    /// Reads UTF-8 content by its logical path from the mounted pack.
    pub fn content_text(&self, path: &AssetPath) -> Result<&str, StoreError> {
        Ok(self.content.load_text(path)?)
    }

    /// Reads content bytes by their logical path from the mounted pack.
    pub fn content_bytes(&self, path: &AssetPath) -> Result<&[u8], StoreError> {
        Ok(self.content.load(path)?)
    }

    /// Returns the mounted entry hash used to detect pack reloads.
    pub fn content_hash(&self, path: &AssetPath) -> Result<u64, StoreError> {
        Ok(self.content.hash(path)?)
    }

    /// Returns the currently mounted immutable pack for content systems and reload handlers.
    #[must_use]
    pub fn content_pack(&self) -> &ContentPack {
        &self.content
    }

    /// Starts loading an asset kind by its validated logical path.
    pub fn load<T: AssetKind>(
        &mut self,
        pool: &JobPool,
        path: AssetPath,
    ) -> Result<Handle<T>, StoreError> {
        let bytes = self.content.load(&path)?.to_vec();
        let source_hash = self.content.hash(&path)?;
        self.assets_mut::<T>()?.load(pool, path, bytes, source_hash)
    }

    /// Adds a store-owned reference and returns another typed handle.
    pub fn retain<T: AssetKind>(&mut self, handle: &Handle<T>) -> Result<Handle<T>, StoreError> {
        self.assets_mut::<T>()?.retain(handle)
    }

    /// Releases one typed handle. Slots are freed during [`apply_loaded`](Self::apply_loaded).
    pub fn release<T: AssetKind>(&mut self, handle: Handle<T>) -> Result<(), StoreError> {
        self.assets_mut::<T>()?.release(handle)
    }

    /// Returns the load status for a typed handle.
    pub fn status<T: AssetKind>(
        &self,
        handle: &Handle<T>,
    ) -> Result<AssetStatus<'_, T>, StoreError> {
        self.assets::<T>()?.status(handle)
    }

    /// Returns the last good value, including after a failed reload.
    pub fn current<T: AssetKind>(&self, handle: &Handle<T>) -> Result<Option<&T>, StoreError> {
        self.assets::<T>()?.current(handle)
    }

    /// Returns the current value identified by a ready store event.
    ///
    /// The type and generation checks keep delayed renderer work from reading a reused slot.
    pub fn current_by_id<T: AssetKind>(&self, id: AssetId) -> Result<Option<&T>, StoreError> {
        if id.kind != TypeId::of::<T>() {
            return Err(StoreError::StaleHandle);
        }
        self.current(&Handle::new(id.slot, id.generation))
    }

    /// Returns the logical path identified by a live typed asset event.
    pub fn path_by_id<T: AssetKind>(&self, id: AssetId) -> Result<&AssetPath, StoreError> {
        if id.kind != TypeId::of::<T>() {
            return Err(StoreError::StaleHandle);
        }
        self.assets::<T>()?
            .path(&Handle::new(id.slot, id.generation))
    }

    /// Applies completed loads and hot reloads; called once per frame.
    pub fn apply_loaded(&mut self, _pool: &JobPool, now: Instant) -> Result<(), StoreError> {
        self.events.clear();
        for type_id in &self.order {
            let Some(assets) = self.kinds.get_mut(type_id) else {
                return Err(StoreError::KindNotRegistered);
            };
            assets.apply(*type_id, now, &mut self.events)?;
        }
        Ok(())
    }

    /// Store events produced by the last [`AssetStore::apply_loaded`] call.
    #[must_use]
    pub fn events(&self) -> &[StoreEvent] {
        &self.events
    }

    /// Clears the frame's events after the renderer has applied them.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    fn assets<T: AssetKind>(&self) -> Result<&Assets<T>, StoreError> {
        self.kinds
            .get(&TypeId::of::<T>())
            .and_then(|assets| assets.as_any().downcast_ref())
            .ok_or(StoreError::KindNotRegistered)
    }

    fn assets_mut<T: AssetKind>(&mut self) -> Result<&mut Assets<T>, StoreError> {
        self.kinds
            .get_mut(&TypeId::of::<T>())
            .and_then(|assets| assets.as_any_mut().downcast_mut())
            .ok_or(StoreError::KindNotRegistered)
    }
}

impl Default for AssetStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;
    use crate::format::{self, Asset, MeshPack};
    use crate::{AssetPath, ContentPack, PackReader};

    fn content(entries: Vec<(&str, Vec<u8>)>) -> Result<ContentPack, Box<dyn Error>> {
        let entries = entries
            .into_iter()
            .map(|(path, bytes)| Ok((AssetPath::new(path)?, bytes)))
            .collect::<Result<Vec<_>, crate::InvalidContentName>>()?;
        let mut bytes = Vec::new();
        ContentPack::write(entries, &mut bytes)?;
        Ok(PackReader::read(Cursor::new(bytes))?)
    }

    fn empty_pack(source_hash: u64) -> MeshPack {
        MeshPack {
            source_hash,
            meshes: Vec::new(),
            materials: Vec::new(),
            nodes: Vec::new(),
            clips: Vec::new(),
            collision: Vec::new(),
            textures: Vec::new(),
        }
    }

    #[test]
    fn typed_kinds_share_load_retain_release_and_generation_path() -> Result<(), Box<dyn Error>> {
        let pack_path = AssetPath::new("packs/pack.meshpack")?;
        let texture_path = AssetPath::new("textures/texture.ktx2")?;
        let pool = JobPool::new(0)?;
        let mut store = AssetStore::new();
        store.mount_content(
            content(vec![
                (
                    "packs/pack.meshpack",
                    format::encode(&Asset::MeshPack(empty_pack(9)), 9)?,
                ),
                (
                    "textures/texture.ktx2",
                    include_bytes!("../tests/fixtures/uastc.ktx2").to_vec(),
                ),
            ])?,
            &pool,
        )?;
        let pack_handle = store.load::<MeshPack>(&pool, pack_path.clone())?;
        let texture_handle = store.load::<TextureData>(&pool, texture_path)?;
        let same_path_handle = store.load::<MeshPack>(&pool, pack_path)?;
        assert_eq!(pack_handle.slot(), same_path_handle.slot());
        assert_eq!(pack_handle.generation(), same_path_handle.generation());
        let pack_id = AssetId {
            kind: TypeId::of::<MeshPack>(),
            slot: pack_handle.slot(),
            generation: pack_handle.generation(),
        };
        assert!(
            store
                .path_by_id::<MeshPack>(pack_id)?
                .as_str()
                .ends_with("pack.meshpack")
        );
        assert!(matches!(
            store.path_by_id::<TextureData>(pack_id),
            Err(StoreError::StaleHandle)
        ));
        let retained = store.retain(&pack_handle)?;
        store.apply_loaded(&pool, Instant::now())?;
        let ready = store.events();
        assert_eq!(ready.len(), 2);
        assert!(
            ready
                .iter()
                .all(|event| matches!(event, StoreEvent::Ready(_)))
        );
        assert!(matches!(
            store.status(&pack_handle),
            Ok(AssetStatus::Ready(_))
        ));
        assert!(matches!(
            store.status(&texture_handle),
            Ok(AssetStatus::Ready(_))
        ));
        store.release(pack_handle)?;
        store.release(same_path_handle)?;
        store.release(retained)?;
        store.release(texture_handle)?;
        store.apply_loaded(&pool, Instant::now())?;
        let events = store.events();
        assert_eq!(events.len(), 2);
        Ok(())
    }

    #[test]
    fn hot_reload_swaps_valid_data_and_keeps_last_good_after_failure() -> Result<(), Box<dyn Error>>
    {
        let path = AssetPath::new("reload.meshpack")?;
        let pool = JobPool::new(0)?;
        let mut store = AssetStore::new();
        store.mount_content(
            content(vec![(
                "reload.meshpack",
                format::encode(&Asset::MeshPack(empty_pack(1)), 1)?,
            )])?,
            &pool,
        )?;
        let handle = store.load::<MeshPack>(&pool, path)?;
        let start = Instant::now();
        store.apply_loaded(&pool, start)?;
        let ready = store.events();
        assert_eq!(ready.len(), 1);
        assert!(matches!(ready.first(), Some(StoreEvent::Ready(_))));
        assert_eq!(
            store.current(&handle)?.map(|value| value.source_hash),
            Some(1)
        );

        store.mount_content(
            content(vec![("reload.meshpack", b"broken cooked data".to_vec())])?,
            &pool,
        )?;
        store.apply_loaded(&pool, start + Duration::from_millis(300))?;
        let failure = match store.status(&handle)? {
            AssetStatus::Failed(failure) => failure,
            _ => return Err(std::io::Error::other("broken reload did not report failure").into()),
        };
        assert!(matches!(
            failure.error(),
            LoadError::Format(crate::format::AssetError::Header)
        ));
        assert_eq!(
            store.current(&handle)?.map(|value| value.source_hash),
            Some(1)
        );

        store.mount_content(
            content(vec![(
                "reload.meshpack",
                format::encode(&Asset::MeshPack(empty_pack(2)), 2)?,
            )])?,
            &pool,
        )?;
        store.apply_loaded(&pool, start + Duration::from_millis(900))?;
        let mut reloaded = store.events().to_vec();
        store.apply_loaded(&pool, start + Duration::from_millis(1200))?;
        reloaded.extend_from_slice(store.events());
        assert_eq!(reloaded.len(), 1);
        assert!(matches!(reloaded.first(), Some(StoreEvent::Reloaded(_))));
        assert_eq!(
            store.current(&handle)?.map(|value| value.source_hash),
            Some(2)
        );
        store.release(handle)?;
        store.apply_loaded(&pool, start + Duration::from_millis(1500))?;
        Ok(())
    }

    #[test]
    fn worker_thread_completes_a_typed_background_load() -> Result<(), Box<dyn Error>> {
        let path = AssetPath::new("worker.meshpack")?;
        let pool = JobPool::new(1)?;
        let mut store = AssetStore::new();
        store.mount_content(
            content(vec![(
                "worker.meshpack",
                format::encode(&Asset::MeshPack(empty_pack(5)), 5)?,
            )])?,
            &pool,
        )?;
        let handle = store.load::<MeshPack>(&pool, path)?;
        for attempt in 0..10_000_u32 {
            store.apply_loaded(
                &pool,
                Instant::now() + Duration::from_millis(u64::from(attempt)),
            )?;
            if matches!(store.status(&handle), Ok(AssetStatus::Ready(_))) {
                break;
            }
            std::thread::yield_now();
        }
        assert!(matches!(store.status(&handle), Ok(AssetStatus::Ready(_))));
        store.release(handle)?;
        store.apply_loaded(&pool, Instant::now() + Duration::from_secs(1))?;
        Ok(())
    }
}
