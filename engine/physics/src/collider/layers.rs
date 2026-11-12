//! Collision membership masks and the game-owned surface identifier stored with each collider.
//!
//! Two colliders interact only when each one's member bits and collision mask accept the other
//! (Rapier's `And` test mode).

use rapier3d::prelude::{InteractionGroups, InteractionTestMode};
use serde::{Deserialize, Serialize};

/// Bit set used for collision membership and query selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct LayerMask(u32);
impl LayerMask {
    /// No layers.
    pub const NONE: Self = Self(0);
    /// Every layer bit.
    pub const ALL: Self = Self(u32::MAX);
    /// Creates a layer mask from its bit representation.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    /// Returns the bit representation.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}
impl std::ops::BitOr for LayerMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitAnd for LayerMask {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// Collision participation masks for one collider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionLayers {
    /// Layers this collider belongs to.
    pub member: LayerMask,
    /// Layers this collider responds to.
    pub collides_with: LayerMask,
}
impl CollisionLayers {
    /// All colliders interact by default.
    pub const ALL: Self = Self {
        member: LayerMask::ALL,
        collides_with: LayerMask::ALL,
    };
    pub(crate) fn rapier(self) -> InteractionGroups {
        InteractionGroups::new(
            self.member.bits().into(),
            self.collides_with.bits().into(),
            InteractionTestMode::And,
        )
    }
}
impl Default for CollisionLayers {
    fn default() -> Self {
        Self::ALL
    }
}

/// Game-defined physical material identifier stored in each collider's user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SurfaceId(pub u16);
impl SurfaceId {
    /// Default surface for generic engine geometry.
    pub const DEFAULT: Self = Self(0);
}
