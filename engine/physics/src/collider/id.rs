//! Collider identifiers, and the owner record that links a collider to its parent body.
//!
//! `ColliderId` is generation-checked, so an id for a removed collider never names a reused slot.

use crate::BodyId;
use rapier3d::prelude::ColliderHandle;
use serde::{Deserialize, Serialize};

/// Generation-checked identifier for a collider in a Rapier arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColliderId {
    index: u32,
    generation: u32,
}
impl ColliderId {
    pub(crate) fn from_handle(handle: ColliderHandle) -> Self {
        let (index, generation) = handle.into_raw_parts();
        Self { index, generation }
    }
    pub(crate) fn handle(self) -> ColliderHandle {
        ColliderHandle::from_raw_parts(self.index, self.generation)
    }
    /// Returns the arena slot portion of this identifier.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
    /// Returns the generation portion of this identifier.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Identifies a collider and its optional parent body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColliderOwner {
    /// Collider that was hit or inspected.
    pub collider: ColliderId,
    /// Parent rigid body, when attached.
    pub body: Option<BodyId>,
}
