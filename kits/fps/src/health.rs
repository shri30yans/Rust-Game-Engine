//! Hit points, teams and damage.
//!
//! [`Health`] is a component that can only be built through validated constructors, so its
//! current value always lies in `[0, max]`. Nothing changes health directly: weapons and lethal
//! movers write a [`DamageEvent`], and [`damage_system`] applies this tick's hits in write order
//! and writes a [`Damaged`] event for each and a [`Killed`] event for each death. The game reads
//! those events for hit markers, kill feeds and scores.
//!
//! Invariants: `1 <= max` and `0 <= current <= max`; damage is a `u32` and saturates at zero; an
//! entity is alive exactly when its current health is above zero; one death writes exactly one
//! [`Killed`], because only the hit that takes health from above zero to zero reports a death.

use engine_world::{NetId, SystemContext, World, WorldError};
use serde::{Deserialize, Serialize};

use crate::error::KitError;
use crate::events::{DamageEvent, Damaged, Killed};
use crate::validate::at_least_one;

/// An amount of damage in whole hit points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Damage(u32);

impl Damage {
    /// Damage of `amount` hit points.
    #[must_use]
    pub const fn new(amount: u32) -> Self {
        Self(amount)
    }

    /// The amount, in hit points.
    #[must_use]
    pub const fn amount(self) -> u32 {
        self.0
    }
}

/// What a hit did to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The target is still alive with `remaining` hit points.
    Survived {
        /// Hit points left after the hit, above zero.
        remaining: u32,
    },
    /// The hit took the target from alive to dead.
    Died,
}

/// Hit points of one actor. The invariant `1 <= max` and `0 <= current <= max` holds for every
/// value, including those read back from a saved state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HealthRaw")]
pub struct Health {
    current: u32,
    max: u32,
}

/// The unvalidated shape of [`Health`], used only to decode it.
#[derive(Deserialize)]
struct HealthRaw {
    current: u32,
    max: u32,
}

impl TryFrom<HealthRaw> for Health {
    type Error = KitError;

    fn try_from(raw: HealthRaw) -> Result<Self, Self::Error> {
        let max = at_least_one(raw.max, "max health")?;
        if raw.current <= max {
            Ok(Self {
                current: raw.current,
                max,
            })
        } else {
            Err(KitError::CountOutOfRange {
                field: "current health",
                requirement: "no more than the max health",
                value: raw.current,
            })
        }
    }
}

impl Health {
    /// Full health of `max` hit points.
    ///
    /// # Errors
    ///
    /// Returns [`KitError::CountOutOfRange`] when `max` is zero.
    pub fn full(max: u32) -> Result<Self, KitError> {
        let max = at_least_one(max, "max health")?;
        Ok(Self { current: max, max })
    }

    /// Hit points left, in `[0, max]`.
    #[must_use]
    pub const fn current(self) -> u32 {
        self.current
    }

    /// The most hit points this actor can have.
    #[must_use]
    pub const fn max(self) -> u32 {
        self.max
    }

    /// Whether any hit points remain.
    #[must_use]
    pub const fn is_alive(self) -> bool {
        self.current > 0
    }

    /// Removes `damage` from the current hit points, never below zero, and reports the outcome.
    pub fn apply(&mut self, damage: Damage) -> Outcome {
        let was_alive = self.is_alive();
        self.current = self.current.saturating_sub(damage.amount());
        match (was_alive, self.is_alive()) {
            (true, false) => Outcome::Died,
            _ => Outcome::Survived {
                remaining: self.current,
            },
        }
    }
}

/// Whether the entity `id` may still act. An entity with no [`Health`] cannot die, so it is alive.
///
/// # Errors
///
/// Returns a world error when `id` does not name a live entity.
pub(crate) fn is_alive(world: &World, id: NetId) -> Result<bool, WorldError> {
    match world.get::<Health>(id) {
        Ok(health) => Ok(health.is_alive()),
        Err(WorldError::MissingComponent(..)) => Ok(true),
        Err(error) => Err(error),
    }
}

/// Which side an actor is on. Friendly fire is decided from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team(u8);

impl Team {
    /// A team with the given number. Teams are compared for equality only.
    #[must_use]
    pub const fn new(number: u8) -> Self {
        Self(number)
    }

    /// The team number.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }
}

/// Applies every hit queued this tick ([`DamageEvent`]s, in write order) to its target's
/// [`Health`], writing a [`Damaged`] event for each and a [`Killed`] event for each hit that takes
/// its target from alive to dead. It never clears a log: the schedule does, before each tick.
///
/// # Errors
///
/// Returns a world error when an event log is missing, or a target is not a live entity with
/// [`Health`].
pub fn damage_system(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let count = world.events::<DamageEvent>()?.len();
    for index in 0..count {
        let Some(hit) = world.events::<DamageEvent>()?.get(index).copied() else {
            break;
        };
        let outcome = world.get_mut::<Health>(hit.target())?.apply(hit.amount());
        world.send(Damaged { hit, outcome })?;
        if outcome == Outcome::Died {
            world.send(Killed {
                victim: hit.target(),
                cause: hit.source(),
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn full_health_starts_at_max() -> Result<(), KitError> {
        let health = Health::full(100)?;
        assert_eq!(health.current(), 100);
        assert!(health.is_alive());
        Ok(())
    }

    #[test]
    fn zero_max_is_rejected() {
        assert!(Health::full(0).is_err());
    }

    #[test]
    fn lethal_damage_kills_once_and_saturates_at_zero() -> Result<(), KitError> {
        let mut health = Health::full(30)?;
        assert_eq!(
            health.apply(Damage::new(10)),
            Outcome::Survived { remaining: 20 }
        );
        assert_eq!(health.apply(Damage::new(50)), Outcome::Died);
        assert_eq!(health.current(), 0);
        assert!(!health.is_alive());
        assert_eq!(
            health.apply(Damage::new(5)),
            Outcome::Survived { remaining: 0 }
        );
        Ok(())
    }

    #[test]
    fn decoding_rejects_current_above_max() {
        let decoded = Health::try_from(HealthRaw {
            current: 10,
            max: 5,
        });
        assert!(decoded.is_err());
    }

    #[test]
    fn decoding_rejects_zero_max() {
        let decoded = Health::try_from(HealthRaw { current: 0, max: 0 });
        assert!(decoded.is_err());
    }

    #[test]
    fn decoding_accepts_a_valid_pair() -> Result<(), KitError> {
        let health = Health::try_from(HealthRaw { current: 3, max: 5 })?;
        assert_eq!(health.current(), 3);
        assert_eq!(health.max(), 5);
        Ok(())
    }

    proptest! {
        #[test]
        fn current_stays_within_zero_and_max(
            max in 1u32..500,
            hits in proptest::collection::vec(0u32..200, 0..24),
        ) {
            let mut health = Health::full(max).map_err(|e| TestCaseError::fail(e.to_string()))?;
            for amount in hits {
                health.apply(Damage::new(amount));
                prop_assert!(health.current() <= health.max());
            }
        }

        #[test]
        fn damage_never_heals(
            max in 1u32..500,
            first in 0u32..200,
            second in 0u32..200,
        ) {
            let mut health = Health::full(max).map_err(|e| TestCaseError::fail(e.to_string()))?;
            health.apply(Damage::new(first));
            let after_first = health.current();
            health.apply(Damage::new(second));
            prop_assert!(health.current() <= after_first);
        }
    }
}
