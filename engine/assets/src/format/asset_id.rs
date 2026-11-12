//! Stable identifiers for source assets, independent of paths and in-memory handles.

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

/// A persistent, non-zero 64-bit identifier assigned to a source asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AssetId(u64);

/// Error returned when a stable asset identifier is zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("asset id must be non-zero")]
pub struct InvalidAssetId;

impl AssetId {
    /// Creates a stable asset identifier.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidAssetId`] when `value` is zero.
    pub fn new(value: u64) -> Result<Self, InvalidAssetId> {
        (value != 0).then_some(Self(value)).ok_or(InvalidAssetId)
    }

    /// The underlying 64-bit identifier.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for AssetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Where a stable source asset was cooked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetLocation {
    /// Mesh-pack filename relative to the cooked directory.
    pub pack: String,
    /// Mesh-pack node index for an object asset; absent for the source-file asset itself.
    pub node: Option<u32>,
    /// Hash of the exact source file bytes used for this cooked result.
    pub content_hash: u64,
}

/// The deterministic stable-ID index written beside cooked assets.
pub type AssetIndex = std::collections::BTreeMap<AssetId, AssetLocation>;
