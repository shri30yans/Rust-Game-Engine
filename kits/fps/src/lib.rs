//! kit-fps: optional first-person shooter gameplay for any game that wants it.
//!
//! The kit provides four parts, all deterministic and all configured by the game through plain
//! parameter structs that are validated on construction:
//!
//! - [`controller`]: first-person locomotion with jumping, coyote time, jump buffering and variable
//!   jump height, driven through the physics character controller;
//! - [`health`]: hit points, teams and the damage pass;
//! - [`events`]: the per-tick events the kit writes (shots, hits, damage, deaths, movement changes),
//!   which the game reads for hit markers, kill feeds, sound and rules;
//! - [`weapon`]: hitscan weapons with ammunition, reload, fire rate and spread;
//! - [`bot`]: computer-controlled actors that see, chase, aim and shoot, using the same
//!   [`MoveCommand`] as the player.
//!
//! The kit names no game, mode, map or weapon. A weapon is data: a game builds a
//! [`WeaponConfig`] from its own numbers.
//!
//! Setup: call [`install`] once on a world to register the components, resources and event logs,
//! then insert the game's [`Motor`], [`engine_physics::PhysicsWorld`] and (when there are bots)
//! [`Waypoints`]. [`add_systems`] schedules the systems in their required order. The order is only
//! what the data flow needs: a system that writes an event runs before the systems that read it,
//! and readers of the same events may run in any order, because reading never consumes.
//!
//! Invariants: every constructor validates its input, so an invalid configuration cannot be built,
//! including when it is decoded from saved state; all randomness comes from a [`engine_core::SimRng`]
//! stream owned by a system or component; config replacement keeps live actor state; no system
//! reads the wall clock.

#![forbid(unsafe_code)]

pub mod aim_assist;
pub mod bot;
pub mod combat_stats;
pub mod command;
pub mod controller;
pub mod error;
pub mod events;
pub mod health;
pub mod movement;
pub mod waypoints;
pub mod weapon;

mod roster;
mod validate;

use engine_world::{Replication, Schedule, SystemFn, World, WorldError};

pub use aim_assist::{AimAssist, AimAssistError, AimAssistParams, AimTarget};
pub use bot::{
    Bot, BotBrain, BotConfig, BotInput, BotParams, Phase, PhysicsSenses, Senses, Target, bot_system,
};
pub use combat_stats::{CombatStats, combat_stats_system};
pub use command::{MoveCommand, Trigger, look_direction};
pub use controller::{
    ControllerConfig, ControllerTuning, Locomotion, LocomotionState, Motor, locomotion_system,
    move_one, step_velocity,
};
pub use error::KitError;
pub use events::{DamageEvent, Damaged, Killed, ShotFired};
pub use health::{Damage, Health, Outcome, Team, damage_system};
pub use movement::{MoveState, MovementEvent, TransitionFacts, next_state};
pub use waypoints::{LinkParams, LinkRules, WaypointId, Waypoints};
pub use weapon::{
    DamageFalloff, MAX_PELLETS, Shot, Weapon, WeaponConfig, WeaponParams, WeaponState,
    weapon_system,
};

/// Registers the kit's components with `world`, inserts its resources and registers its event logs,
/// sized for `max_actors` actors (from the game's data: the most actors a match can hold).
///
/// Call once per world, before any entity carrying these components is spawned.
///
/// # Errors
///
/// Returns a world error when a component, resource or event log is already registered.
pub fn install(world: &mut World, max_actors: usize) -> Result<(), WorldError> {
    world.register::<Team>(Replication::Replicated)?;
    world.register::<Health>(Replication::Replicated)?;
    world.register::<Locomotion>(Replication::Replicated)?;
    world.register::<Weapon>(Replication::Replicated)?;
    world.register::<Bot>(Replication::ServerOnly)?;
    world.insert_resource(roster::Roster::new())?;
    events::register(world, max_actors)
}

/// Adds the kit's systems to `schedule` in their required order: bots decide, locomotion moves
/// the bodies, the game's `physics_step` advances physics, weapons resolve shots, and damage is
/// applied.
///
/// # Errors
///
/// Returns a world error when one of the systems is already scheduled.
pub fn add_systems(schedule: &mut Schedule, physics_step: SystemFn) -> Result<(), WorldError> {
    schedule.add_system("fps.bot", bot_system)?;
    schedule.add_system("fps.locomotion", locomotion_system)?;
    schedule.add_system("physics.step", physics_step)?;
    schedule.add_system("fps.weapon", weapon_system)?;
    schedule.add_system("fps.damage", damage_system)?;
    Ok(())
}
