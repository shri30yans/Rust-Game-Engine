//! Keeps a validated level synchronized with its logical entry in the mounted content pack.

use engine_assets::{AssetPath, ContentPack};
use engine_core::StateHasher;
use engine_world::Resource;

use crate::{Level, SceneError};

/// Result of checking one mounted level entry.
#[derive(Debug)]
pub enum Reload {
    /// The pack still contains the last accepted level bytes.
    Unchanged,
    /// Changed bytes parsed and validated as a replacement level.
    Applied,
    /// Changed bytes were rejected; the last good level remains available.
    Rejected(SceneError),
}

/// A validated level and the logical pack entry that produced it.
pub struct Watched<Kind> {
    path: AssetPath,
    hash: u64,
    value: Kind,
}

impl Watched<Level> {
    /// Loads a level from one logical path in the mounted game pack.
    pub fn level(path: AssetPath, content: &ContentPack) -> Result<Self, SceneError> {
        let hash = content
            .hash(&path)
            .map_err(|error| SceneError::AssetIndex {
                path: path.as_str().into(),
                reason: error.to_string(),
            })?;
        let value = load_level(content, &path)?;
        Ok(Self { path, hash, value })
    }

    /// The last accepted validated level.
    #[must_use]
    pub const fn value(&self) -> &Level {
        &self.value
    }

    /// Reloads only when this pack entry's validated hash changes.
    pub fn poll(&mut self, content: &ContentPack) -> Reload {
        let hash = match content.hash(&self.path) {
            Ok(hash) => hash,
            Err(error) => {
                return Reload::Rejected(SceneError::AssetIndex {
                    path: self.path.as_str().into(),
                    reason: error.to_string(),
                });
            }
        };
        if hash == self.hash {
            return Reload::Unchanged;
        }
        match load_level(content, &self.path) {
            Ok(level) => {
                self.value = level;
                self.hash = hash;
                Reload::Applied
            }
            Err(error) => Reload::Rejected(error),
        }
    }
}

fn load_level(content: &ContentPack, path: &AssetPath) -> Result<Level, SceneError> {
    let text = content
        .load_text(path)
        .map_err(|error| SceneError::Tuning {
            file: path.as_str().to_owned(),
            reason: error.to_string(),
        })?;
    Level::parse(text, path.as_str())
}

/// Current authored level retained in a game world for engine-owned live diffs.
#[derive(Default)]
pub struct WatchedLevel {
    watched: Option<Watched<Level>>,
}

impl WatchedLevel {
    /// Wraps a watched pack level for insertion as a world resource.
    #[must_use]
    pub const fn new(watched: Watched<Level>) -> Self {
        Self {
            watched: Some(watched),
        }
    }

    /// Takes the watcher while the app polls it alongside other world resources.
    pub fn take(&mut self) -> Option<Watched<Level>> {
        self.watched.take()
    }

    /// Restores the watcher after the frame-boundary poll.
    pub fn restore(&mut self, watched: Watched<Level>) {
        self.watched = Some(watched);
    }
}

impl Resource for WatchedLevel {
    fn hash_into(&self, _hasher: &mut StateHasher) {}
}
