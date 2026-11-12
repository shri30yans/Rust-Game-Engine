//! Staging, readiness checks, and commit of watched scene edits.
use std::time::Instant;

use engine_assets::AssetStore;
use engine_assets::format::MeshPack;
use engine_assets::store::AssetStatus;
use engine_scene::{Level, Reload, SceneError, WatchedLevel};
use engine_world::{World, WorldError};
use jobs::JobPool;
use platform::log_error;
use render::{Gfx, Ui};

use super::state::{LiveScene, PendingLevel, PendingVisualAsset};
use super::{LevelDiff, apply_level_diff, diff_levels};
use crate::asset_uploader::{AssetUploader, UploadStatus};
use crate::error::AppError;

const BANNER_FONT: &[u8] = include_bytes!("../../../render/fonts/Roboto-Medium.ttf");

impl LiveScene {
    /// Clears state tied to the old world after a successful travel.
    ///
    /// # Errors
    ///
    /// Returns an asset-store error when a staged reference cannot be released.
    pub fn reset(&mut self, store: &mut AssetStore) -> Result<(), AppError> {
        Self::release_assets(store, &mut self.release_after_present)?;
        if let Some(pending) = self.pending.take() {
            let mut assets = pending.assets;
            Self::release_assets(store, &mut assets)?;
        }
        self.current = None;
        self.banner.clear();
        Ok(())
    }

    /// Uploads the banner font. Call once, from the client's renderer setup.
    ///
    /// # Errors
    ///
    /// Returns the renderer's error when the font cannot be added.
    pub fn setup(&mut self, gfx: &mut Gfx) -> Result<(), AppError> {
        self.font = Some(gfx.ui.atlas.add_font(BANNER_FONT)?);
        Ok(())
    }

    /// Stages a watched edit and commits it only after its cooked visuals are GPU-ready.
    ///
    /// This must run before [`AssetUploader::update`] each frame so every newly staged soft path
    /// can participate in that frame's normal load and upload work. It returns a committed level
    /// so the game can rebuild derived scene data at the same frame boundary.
    ///
    /// # Errors
    ///
    /// Returns a world or asset-store error when the client-owned state cannot be accessed.
    pub fn poll(
        &mut self,
        world: &mut World,
        content: &engine_scene::SceneContent,
        store: &mut AssetStore,
        uploader: &AssetUploader,
        pool: &JobPool,
    ) -> Result<Option<Level>, AppError> {
        self.poll_at(world, content, store, uploader, pool, Instant::now())
    }

    /// [`LiveScene::poll`] with the clock passed in, so tests can drive it.
    pub(super) fn poll_at(
        &mut self,
        world: &mut World,
        content: &engine_scene::SceneContent,
        store: &mut AssetStore,
        uploader: &AssetUploader,
        pool: &JobPool,
        now: Instant,
    ) -> Result<Option<Level>, AppError> {
        let content = content.with_pack(store.content_pack().clone());
        Self::release_assets(store, &mut self.release_after_present)?;
        if world.resource::<WatchedLevel>().is_err() {
            return Ok(None);
        }
        self.banner.clear_expired(now);
        let mut watched = world
            .resource_mut::<WatchedLevel>()?
            .take()
            .ok_or(WorldError::Invariant("the watched level is already taken"))?;
        if self.current.is_none() {
            self.current = Some(watched.value().clone());
        }
        let staged = match watched.poll(store.content_pack()) {
            Reload::Unchanged => Ok(()),
            Reload::Applied => self.stage(watched.value().clone(), &content, store, pool, now),
            Reload::Rejected(error) => {
                self.report(&error, now);
                Ok(())
            }
        };
        world.resource_mut::<WatchedLevel>()?.restore(watched);
        staged?;
        self.commit_ready(world, &content, store, uploader, now)
    }

    fn stage(
        &mut self,
        level: Level,
        content: &engine_scene::SceneContent,
        store: &mut AssetStore,
        pool: &JobPool,
        now: Instant,
    ) -> Result<(), AppError> {
        self.discard_pending(store)?;
        let diff = self
            .current
            .as_ref()
            .map_or_else(LevelDiff::default, |previous| diff_levels(previous, &level));
        let mut records = Vec::with_capacity(diff.replaced.len() + diff.added.len());
        records.extend(diff.replaced.iter().cloned());
        records.extend(diff.added.iter().cloned());
        let requirements =
            match engine_scene::resolve_visual_assets(&records, store.content_pack(), content) {
                Ok(requirements) => requirements,
                Err(error) => {
                    self.report(&error, now);
                    return Ok(());
                }
            };
        let mut assets = Vec::with_capacity(requirements.len());
        for requirement in requirements {
            match store.load::<MeshPack>(pool, requirement.pack_path().clone()) {
                Ok(handle) => assets.push(PendingVisualAsset {
                    source_hash: requirement.source_hash(),
                    handle,
                }),
                Err(error) => {
                    Self::release_assets(store, &mut assets)?;
                    self.report_text(error.to_string(), now);
                    return Ok(());
                }
            }
        }
        self.pending = Some(PendingLevel {
            level,
            diff,
            assets,
            failure_reported: false,
        });
        Ok(())
    }

    fn commit_ready(
        &mut self,
        world: &mut World,
        content: &engine_scene::SceneContent,
        store: &mut AssetStore,
        uploader: &AssetUploader,
        now: Instant,
    ) -> Result<Option<Level>, AppError> {
        let readiness = self.pending.as_mut().map(|pending| {
            let mut failed = None;
            let ready = pending
                .assets
                .iter()
                .all(|asset| match store.status(&asset.handle) {
                    Ok(AssetStatus::Ready(pack)) if pack.source_hash == asset.source_hash => {
                        match uploader.status(&asset.handle) {
                            UploadStatus::Ready(gpu) => gpu.source_hash == asset.source_hash,
                            UploadStatus::Pending => false,
                            UploadStatus::Failed(error) => {
                                failed = Some(error.to_string());
                                false
                            }
                        }
                    }
                    Ok(AssetStatus::Ready(pack)) => {
                        failed = Some(format!(
                            "cooked pack revision {} does not match required revision {}",
                            pack.source_hash, asset.source_hash
                        ));
                        false
                    }
                    Ok(AssetStatus::Loading) => false,
                    Ok(AssetStatus::Failed(error)) => {
                        failed = Some(error.to_string());
                        false
                    }
                    Err(error) => {
                        failed = Some(error.to_string());
                        false
                    }
                });
            (ready, failed)
        });
        let Some((ready, failed)) = readiness else {
            return Ok(None);
        };
        if let Some(error) = failed {
            if let Some(pending) = self.pending.as_mut()
                && !pending.failure_reported
            {
                pending.failure_reported = true;
                self.report_text(error, now);
            }
            return Ok(None);
        }
        if !ready {
            return Ok(None);
        }
        let pending = self
            .pending
            .take()
            .ok_or(WorldError::Invariant("the pending live level is missing"))?;
        match apply_level_diff(world, &pending.diff, &pending.level, content) {
            Ok(()) => {
                self.current = Some(pending.level.clone());
                self.release_after_present = pending.assets;
                self.banner.clear();
                Ok(Some(pending.level))
            }
            Err(error) => {
                let mut assets = pending.assets;
                Self::release_assets(store, &mut assets)?;
                self.report(&error, now);
                Ok(None)
            }
        }
    }

    fn discard_pending(&mut self, store: &mut AssetStore) -> Result<(), AppError> {
        if let Some(pending) = self.pending.take() {
            let mut assets = pending.assets;
            Self::release_assets(store, &mut assets)?;
        }
        Ok(())
    }

    fn release_assets(
        store: &mut AssetStore,
        assets: &mut Vec<PendingVisualAsset>,
    ) -> Result<(), AppError> {
        for asset in assets.drain(..) {
            store.release(asset.handle)?;
        }
        Ok(())
    }

    /// Logs an error and shows it in the banner.
    fn report(&mut self, error: &SceneError, now: Instant) {
        self.report_text(error.to_string(), now);
    }

    fn report_text(&mut self, text: String, now: Instant) {
        log_error(&text);
        self.banner.show(text, now);
    }

    /// The message the banner shows, if any.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.banner.message()
    }

    /// Draws the banner across `screen_width` pixels, when a message is set. Draws nothing before
    /// [`LiveScene::setup`].
    pub fn draw(&self, ui: &mut Ui, screen_width: f32) {
        if let Some(font) = self.font {
            self.banner.draw(ui, screen_width, font);
        }
    }
}
