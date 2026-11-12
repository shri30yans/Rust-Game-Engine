//! Hit points, teams and damage.
//!
//! [`Health`] is a component that can only be built through validated constructors, so its
//! current value always lies in `[0, max]`. Weapons never change health directly: they queue a
//! [`DamageEvent`] in the [`DamageLog`] resource, and [`damage_system`] applies the queue once per
//! tick and records every death in the same log. The game reads the log for hit markers and kill
//! feeds, and it is cleared at the start of the next tick's damage pass.
//!
//! Invariants: `1 <= max` and `0 <= current <= max`; damage is a `u32` and saturates at zero; an
//! entity is alive exactly when its current health is above zero; the log contents enter the state
//! hash, so a queue left behind by a schedule that skipped [`damage_system`] changes the hash
//! instead of hiding.

use engine_core::StateHasher;
use engine_world::{NetId, Resource, SystemContext, World, WorldError};
use serde::{Deserialize, Serialize};

use crate::error::KitError;
use crate::validate::at_least_one;

/// Capacity reserved for the damage log at install. More events grow the buffer once.
pub(crate) const DAMAGE_LOG_CAPACITY: usize = 64;

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

/// One hit waiting to be applied: who was hit, who hit them, and how much damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageEvent {
    target: NetId,
    source: NetId,
    amount: Damage,
}

impl DamageEvent {
    /// A hit on `target` by `source` for `amount`.
    #[must_use]
    pub const fn new(target: NetId, source: NetId, amount: Damage) -> Self {
        Self {
            target,
            source,
            amount,
        }
    }

    /// The entity that was hit.
    #[must_use]
    pub const fn target(self) -> NetId {
        self.target
    }

    /// The entity that dealt the hit.
    #[must_use]
    pub const fn source(self) -> NetId {
        self.source
    }

    /// The damage dealt.
    #[must_use]
    pub const fn amount(self) -> Damage {
        self.amount
    }
}

/// The hits queued for [`damage_system`], and the deaths it recorded. Owned by the world as a
/// resource; the game reads it after each tick.
pub struct DamageLog {
    events: Vec<DamageEvent>,
    deaths: Vec<NetId>,
}

impl DamageLog {
    /// An empty log with [`DAMAGE_LOG_CAPACITY`] reserved for each list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(DAMAGE_LOG_CAPACITY),
            deaths: Vec::with_capacity(DAMAGE_LOG_CAPACITY),
        }
    }

    /// Queues a hit for the next [`damage_system`] run.
    pub fn push(&mut self, event: DamageEvent) {
        self.events.push(event);
    }

    /// The hits queued since the last damage pass, in the order they were pushed.
    #[must_use]
    pub fn events(&self) -> &[DamageEvent] {
        &self.events
    }

    /// The entities that died in the last damage pass, in the order they were hit.
    #[must_use]
    pub fn deaths(&self) -> &[NetId] {
        &self.deaths
    }
}

impl Default for DamageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Resource for DamageLog {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.events.len() as u64);
        for event in &self.events {
            hasher.write_u32(event.amount.amount());
        }
        hasher.write_u64(self.deaths.len() as u64);
    }
}

/// Applies every queued hit to its target's [`Health`] and records each death, then empties the
/// queue. Both lists keep their capacity: they are taken out, drained and handed back, so a tick
/// that stays within capacity allocates nothing.
///
/// # Errors
///
/// Returns a world error when the log is missing, or a target is not a live entity with
/// [`Health`]. The queue is emptied even when a hit fails.
pub fn damage_system(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let mut events = std::mem::take(&mut world.resource_mut::<DamageLog>()?.events);
    let mut deaths = std::mem::take(&mut world.resource_mut::<DamageLog>()?.deaths);
    deaths.clear();
    let result = apply_all(world, &events, &mut deaths);
    events.clear();
    let log = world.resource_mut::<DamageLog>()?;
    log.events = events;
    log.deaths = deaths;
    result
}

/// Applies each queued hit in order, recording deaths, and stops at the first failure.
fn apply_all(
    world: &mut World,
    events: &[DamageEvent],
    deaths: &mut Vec<NetId>,
) -> Result<(), WorldError> {
    for event in events {
        let outcome = world.get_mut::<Health>(event.target)?.apply(event.amount);
        if outcome == Outcome::Died {
            deaths.push(event.target);
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
