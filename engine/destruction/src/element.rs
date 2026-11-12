//! Element specs, immutable element definitions and life-cycle states.
//!
//! [`ElementDef`] is fixed when the structure is built. Health lives beside
//! the state in [`crate::state::StructureState`], so the definition stays
//! immutable. [`ElementState`] is the only life-cycle value the solver changes:
//! `Intact` and `Failing` elements are supported, `Broken` and `Detached` are not.

use std::num::NonZeroU16;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::ids::MaterialId;

/// Authoring data for one element. The builder turns it into an [`ElementDef`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElementSpec {
    /// Material the element is made of. Mass and capacity derive from it.
    pub material: MaterialId,
    /// Volume; mass is `density * volume`. Must be finite and positive.
    pub volume: f32,
    /// Cross-section area; capacity is `strength_per_area * area`. Must be finite and positive.
    pub area: f32,
    /// Position used by blasts. Every component must be finite.
    pub position: Vec3,
    /// Health when undamaged.
    pub max_health: NonZeroU16,
    /// Anchored elements are ground-fixed sources of support and never detach themselves.
    pub anchor: bool,
}

/// Immutable definition of one element, as validated by the builder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElementDef {
    mass: f32,
    capacity: f32,
    max_health: NonZeroU16,
    anchor: bool,
    position: Vec3,
}

impl ElementDef {
    /// Assembles a definition from already-validated parts.
    pub(crate) const fn new(
        mass: f32,
        capacity: f32,
        max_health: NonZeroU16,
        anchor: bool,
        position: Vec3,
    ) -> Self {
        Self {
            mass,
            capacity,
            max_health,
            anchor,
            position,
        }
    }

    /// Mass the element adds to every load it sits under. Always positive.
    #[must_use]
    pub const fn mass(self) -> f32 {
        self.mass
    }

    /// Load the element carries at full health. Always positive.
    #[must_use]
    pub const fn capacity(self) -> f32 {
        self.capacity
    }

    /// Health when undamaged.
    #[must_use]
    pub const fn max_health(self) -> NonZeroU16 {
        self.max_health
    }

    /// True for ground-fixed anchors.
    #[must_use]
    pub const fn is_anchor(self) -> bool {
        self.anchor
    }

    /// Position used by blasts.
    #[must_use]
    pub const fn position(self) -> Vec3 {
        self.position
    }
}

/// Life-cycle state of one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementState {
    /// Supported and not overloaded.
    Intact,
    /// Overloaded. Breaks after `ticks_left` more warning rounds; damage never shortens this.
    Failing {
        /// Warning rounds still to pass before the element breaks. Always at least one.
        ticks_left: u32,
    },
    /// Destroyed. Carries nothing and supports nothing from now on.
    Broken,
    /// Lost every support path to an anchor. Falls as part of a detached group.
    Detached,
}

impl ElementState {
    /// True when the element still takes part in support and load: `Intact` or `Failing`.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Intact | Self::Failing { .. })
    }
}

/// Coarse health reading used in damage events, so clients need not track exact health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthBand {
    /// Above two thirds of maximum health.
    Healthy,
    /// Above one third and at most two thirds of maximum health.
    Cracked,
    /// At most one third of maximum health, including zero.
    Critical,
}

impl HealthBand {
    /// Band for `health` out of `max_health`, computed in integers so the edges are exact.
    #[must_use]
    pub fn of(health: u16, max_health: NonZeroU16) -> Self {
        let scaled = u32::from(health) * 3;
        let max = u32::from(max_health.get());
        if scaled > 2 * max {
            Self::Healthy
        } else if scaled > max {
            Self::Cracked
        } else {
            Self::Critical
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn health_bands_have_exact_edges() {
        let max = NonZeroU16::new(9).unwrap();
        assert_eq!(HealthBand::of(9, max), HealthBand::Healthy);
        assert_eq!(HealthBand::of(7, max), HealthBand::Healthy);
        assert_eq!(HealthBand::of(6, max), HealthBand::Cracked);
        assert_eq!(HealthBand::of(4, max), HealthBand::Cracked);
        assert_eq!(HealthBand::of(3, max), HealthBand::Critical);
        assert_eq!(HealthBand::of(0, max), HealthBand::Critical);
    }

    #[test]
    fn only_intact_and_failing_are_live() {
        assert!(ElementState::Intact.is_live());
        assert!(ElementState::Failing { ticks_left: 1 }.is_live());
        assert!(!ElementState::Broken.is_live());
        assert!(!ElementState::Detached.is_live());
    }
}
