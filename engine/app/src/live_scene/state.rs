//! State retained between live scene polls.
use engine_assets::Handle;
use engine_assets::format::MeshPack;
use engine_scene::Level;
use render::FontId;

use crate::error_banner::ErrorBanner;

use super::LevelDiff;

pub(super) struct PendingVisualAsset {
    pub(super) source_hash: u64,
    pub(super) handle: Handle<MeshPack>,
}

/// One validated level edit waiting for its newly required visual packs.
pub(super) struct PendingLevel {
    pub(super) level: Level,
    pub(super) diff: LevelDiff,
    pub(super) assets: Vec<PendingVisualAsset>,
    pub(super) failure_reported: bool,
}

/// Reloads the world's level when its file changes, and shows the reason when a change is refused.
///
/// A watched edit first becomes a [`PendingLevel`]. The old world remains authoritative while its
/// new cooked visuals load and upload through the client-owned asset path. The diff commits only
/// at a later frame boundary when each exact cooked revision is drawable.
pub struct LiveScene {
    pub(super) current: Option<Level>,
    pub(super) pending: Option<PendingLevel>,
    pub(super) release_after_present: Vec<PendingVisualAsset>,
    pub(super) font: Option<FontId>,
    pub(super) banner: ErrorBanner,
}

impl Default for LiveScene {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveScene {
    /// Creates the engine-owned development level reloader.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: None,
            pending: None,
            release_after_present: Vec::new(),
            font: None,
            banner: ErrorBanner::default(),
        }
    }
}
