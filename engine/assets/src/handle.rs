//! Typed handles identify asset slots without owning asset memory.
//!
//! Refcounts live in [`crate::store::AssetStore`]; callers duplicate ownership through its retain
//! method so handle values stay small and contain no shared pointers.
use std::marker::PhantomData;

/// Slot index and generation for one asset type.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Handle<T> {
    /// Store slot index.
    pub(crate) slot: u32,
    /// Slot generation preventing stale-handle aliasing.
    pub(crate) generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Creates a typed handle for an internal slot.
    pub(crate) const fn new(slot: u32, generation: u32) -> Self {
        Self {
            slot,
            generation,
            marker: PhantomData,
        }
    }

    /// Returns the slot index.
    #[must_use]
    pub const fn slot(&self) -> u32 {
        self.slot
    }

    /// Returns the slot generation.
    #[must_use]
    pub const fn generation(&self) -> u32 {
        self.generation
    }
}
