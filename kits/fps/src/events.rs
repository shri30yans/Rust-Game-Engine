//! The kit's per-tick events: what its systems tell the game and each other.
//!
//! Every type here lives in an engine event log ([`engine_world::Events`]) that [`crate::install`]
//! registers and the schedule empties before each tick. Writers and readers:
//!
//! - [`ShotFired`]: written by [`crate::weapon_system`] for every shot; read by the game's
//!   feedback, hearing and audio, and by [`crate::combat_stats_system`].
//! - [`DamageEvent`]: a hit waiting for the damage pass, written by the weapon system (a pellet
//!   that hit an actor); read by [`crate::damage_system`].
//! - [`Damaged`] and [`Killed`]: written by the damage pass for each applied hit and each death;
//!   read by the game's feedback and rules and by the combat stats.
//! - [`crate::MovementEvent`]: a locomotion mode change (landing is a change out of `Air`), written
//!   by the locomotion system.
//!
//! Invariants: events are plain copies of what happened, so a reader never needs the writer's
//! state; no kit system clears a log.

use engine_world::{NetId, World, WorldError};
use glam::Vec3;

use crate::health::{Damage, Outcome};
use crate::movement::MovementEvent;
use crate::weapon::MAX_PELLETS;

/// One shot leaving a weapon: who fired, from where, along which aim line, and how far it reaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotFired {
    /// The entity that fired.
    pub shooter: NetId,
    /// Where the shot left the shooter's eye.
    pub origin: Vec3,
    /// Unit aim direction (the view, not a sampled pellet).
    pub aim: Vec3,
    /// The weapon's full range, in metres.
    pub range_m: f32,
}

/// One hit waiting to be applied: who was hit, who or what hit them, how much damage, and from where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageEvent {
    target: NetId,
    source: NetId,
    amount: Damage,
    origin: Vec3,
    distance_m: f32,
}

impl DamageEvent {
    /// A hit on `target` by `source` for `amount`, dealt from `origin`, `distance_m` metres away.
    #[must_use]
    pub const fn new(
        target: NetId,
        source: NetId,
        amount: Damage,
        origin: Vec3,
        distance_m: f32,
    ) -> Self {
        Self {
            target,
            source,
            amount,
            origin,
            distance_m,
        }
    }

    /// The entity that was hit.
    #[must_use]
    pub const fn target(self) -> NetId {
        self.target
    }

    /// The entity that dealt the hit: a shooter, or a mover that crushed the target.
    #[must_use]
    pub const fn source(self) -> NetId {
        self.source
    }

    /// The damage dealt.
    #[must_use]
    pub const fn amount(self) -> Damage {
        self.amount
    }

    /// Where the hit came from: the shooter's eye, or the crushing mover's centre.
    #[must_use]
    pub const fn origin(self) -> Vec3 {
        self.origin
    }

    /// Distance from the origin to the target when the hit was dealt, in metres.
    #[must_use]
    pub const fn distance_m(self) -> f32 {
        self.distance_m
    }
}

/// A hit the damage pass applied, and what it did to the target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Damaged {
    /// The hit as it was queued.
    pub hit: DamageEvent,
    /// What the hit did.
    pub outcome: Outcome,
}

/// A death: the hit that took `victim` from alive to dead was dealt by `cause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Killed {
    /// The entity that died.
    pub victim: NetId,
    /// The shooter or the lethal mover that dealt the killing hit.
    pub cause: NetId,
}

/// Registers the kit's event logs, each sized for `max_actors` actors acting every tick: one shot
/// and up to [`MAX_PELLETS`] hits per actor, one death and one movement change per actor.
pub(crate) fn register(world: &mut World, max_actors: usize) -> Result<(), WorldError> {
    let hits = max_actors.saturating_mul(MAX_PELLETS);
    world.add_events::<ShotFired>(max_actors)?;
    world.add_events::<DamageEvent>(hits)?;
    world.add_events::<Damaged>(hits)?;
    world.add_events::<Killed>(max_actors)?;
    world.add_events::<MovementEvent>(max_actors)?;
    Ok(())
}
