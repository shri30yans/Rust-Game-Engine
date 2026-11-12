//! Typed identifiers for structure elements, detached groups and materials.
//!
//! Each id is a newtype over `u32`, so an element index cannot be passed where
//! a group or material index is expected. Ids are minted by the builder and the
//! solver; every public lookup rejects an id that does not name an entry.

use serde::{Deserialize, Serialize};

/// Index of one element within a structure. Dense from zero in build order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ElementId(u32);

impl ElementId {
    /// Wraps a raw element index. Lookups reject indices outside the structure.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Converts to a table position. A value that cannot fit a `usize` maps to
    /// `usize::MAX`, which no table can hold, so every `get` on it returns `None`.
    #[must_use]
    pub(crate) fn index(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }

    /// Creates the id for a table position, or `None` when it exceeds `u32::MAX`.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }
}

/// Identifier of a detached group. Assigned in the order groups detach, so the
/// same inputs always produce the same ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(u32);

impl GroupId {
    /// Wraps a raw group id.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw id.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Index of a material within a builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MaterialId(u32);

impl MaterialId {
    /// Wraps a raw material index. The builder rejects indices it did not issue.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Converts to a table position; unrepresentable values map to `usize::MAX`.
    #[must_use]
    pub(crate) fn index(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }

    /// Creates the id for a table position, or `None` when it exceeds `u32::MAX`.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }
}
