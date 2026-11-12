//! kit-fps: optional first-person shooter gameplay for any game that wants it.
//!
//! The kit provides four parts, all deterministic and all configured by the game through plain
//! parameter structs that are validated on construction:
//!
//! - [`controller`]: first-person locomotion with jumping, coyote time, jump buffering and variable
//!   jump height, driven through the physics character controller;
//! - [`health`]: hit points, teams and the damage queue that the game can read for hit markers and
//!   kill feeds;
//! - [`weapon`]: hitscan weapons with ammunition, reload, fire rate and spread;
//! - [`bot`]: computer-controlled actors that see, chase, aim and shoot, using the same
//!   [`MoveCommand`] as the player.
//!
//! The kit names no game, mode, map or weapon. A weapon is data: a game builds a
//! [`WeaponConfig`] from its own numbers.
//!
//! Setup: call [`install`] once on a world to register the components and resources, then insert
//! the game's [`Motor`], [`engine_physics::PhysicsWorld`] and (when there are bots)
//! [`Waypoints`]. [`add_systems`] schedules the systems in their required order.
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
pub mod health;
pub mod movement;
pub mod waypoints;
pub mod weapon;

mod roster;
mod validate;

use engine_world::{Replication, Schedule, World, WorldError};

pub use aim_assist::{AimAssist, AimAssistError, AimAssistParams, AimTarget};
pub use bot::{
    Bot, BotBrain, BotConfig, BotInput, BotParams, Phase, PhysicsSenses, Senses, Target, bot_system,
};
pub use combat_stats::{CombatStats, combat_stats_system};
pub use command::{MoveCommand, Trigger, look_direction};
pub use controller::{
    ControllerConfig, ControllerTuning, Locomotion, LocomotionState, Motor,
    clear_movement_log_system, locomotion_system, move_one, step_velocity,
};
pub use error::KitError;
pub use health::{Damage, DamageEvent, DamageLog, Health, Outcome, Team, damage_system};
pub use movement::{MoveState, MovementEvent, MovementLog, TransitionFacts, next_state};
pub use waypoints::{LinkParams, LinkRules, WaypointId, Waypoints};
pub use weapon::{
    DamageFalloff, MAX_PELLETS, Shot, Weapon, WeaponConfig, WeaponParams, WeaponState,
    weapon_system,
};

/// Registers the kit's components with `world` and inserts its resources.
///
/// Call once per world, before any entity carrying these components is spawned.
///
/// # Errors
///
/// Returns a world error when a component is already registered or a resource is already present.
pub fn install(world: &mut World) -> Result<(), WorldError> {
    world.register::<Team>(Replication::Replicated)?;
    world.register::<Health>(Replication::Replicated)?;
    world.register::<Locomotion>(Replication::Replicated)?;
    world.register::<Weapon>(Replication::Replicated)?;
    world.register::<Bot>(Replication::ServerOnly)?;
    world.insert_resource(DamageLog::new())?;
    world.insert_resource(MovementLog::with_capacity(32))?;
    world.insert_resource(roster::Roster::new())?;
    Ok(())
}

/// Adds the kit's systems to `schedule` in their required order: bots decide, locomotion moves
/// the bodies, weapons resolve shots, and damage is applied. The physics step belongs after
/// locomotion, so the game schedules it there.
///
/// # Errors
///
/// Returns a world error when one of the systems is already scheduled.
pub fn add_systems(schedule: &mut Schedule) -> Result<(), WorldError> {
    schedule.add_system("fps.movement_log.clear", clear_movement_log_system)?;
    schedule.add_system("fps.bot", bot_system)?;
    schedule.add_system("fps.locomotion", locomotion_system)?;
    schedule.add_system("fps.weapon", weapon_system)?;
    schedule.add_system("fps.damage", damage_system)?;
    Ok(())
}
