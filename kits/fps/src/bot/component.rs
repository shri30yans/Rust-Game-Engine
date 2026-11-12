//! The bot component and its per-tick driver: [`Bot`] holds the configuration, the brain and the
//! bot's own random stream, and [`bot_system`] sets each bot's command for the tick.
//!
//! Invariants: each bot draws from its own stream, forked when it is spawned; the brain and its
//! command are stored together after a tick; a world with bots must have waypoints.

use engine_core::SimRng;
use engine_physics::PhysicsWorld;
use engine_world::{NetId, SystemContext, World, WorldError};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::brain::{BotBrain, BotInput};
use super::config::BotConfig;
use super::senses::{PhysicsSenses, pick_enemy};
use crate::controller::Locomotion;
use crate::error::{KitError, lift};
use crate::health::Team;
use crate::roster::for_each_with;
use crate::validate::finite_vec3;
use crate::waypoints::Waypoints;
use crate::weapon::Weapon;

/// The bot component: its configuration, its brain and its own RNG stream. The stream is forked
/// from the world's setup by the code that spawns the bot, so each bot's draws are its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bot {
    pub(super) config: BotConfig,
    pub(super) brain: BotBrain,
    pub(super) rng: SimRng,
}

impl Bot {
    /// A bot with a fresh brain, drawing from `rng`.
    #[must_use]
    pub const fn new(config: BotConfig, rng: SimRng) -> Self {
        Self {
            config,
            brain: BotBrain::new(),
            rng,
        }
    }

    /// The bot's configuration.
    #[must_use]
    pub const fn config(&self) -> &BotConfig {
        &self.config
    }

    /// Replaces the validated tuning without resetting the bot's brain or random stream.
    pub fn set_config(&mut self, config: BotConfig) {
        self.config = config;
    }

    /// The bot's brain after the last tick.
    #[must_use]
    pub const fn brain(&self) -> &BotBrain {
        &self.brain
    }

    /// Tells the bot where an enemy is last known to be, without sight. See [`BotBrain::alert`].
    ///
    /// # Errors
    ///
    /// Returns [`KitError::NonFiniteVector`] when `position` is not finite.
    pub fn alert(&mut self, position: Vec3) -> Result<(), KitError> {
        let position = finite_vec3(position, "alert position")?;
        self.brain.alert(&self.config, position, &mut self.rng);
        Ok(())
    }
}

/// Decides every bot's command for one tick. Runs before [`crate::controller::locomotion_system`]
/// and [`crate::weapon::weapon_system`], which act on the command it sets.
///
/// Each bot needs a [`Locomotion`], a [`Team`] and a [`Weapon`]. A world with bots needs
/// [`Waypoints`] with at least one point, and a [`engine_physics::PhysicsWorld`] for line of sight.
///
/// # Errors
///
/// Returns a world error when a bot lacks a required component or resource, the waypoint list is
/// empty, or a sense query fails.
pub fn bot_system(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let dt = ctx.dt;
    for_each_with::<Bot, _>(world, |world, id| drive(world, id, dt))
}

/// Decides one bot's command for the tick and stores the new brain and command.
fn drive(world: &mut World, id: NetId, dt: f32) -> Result<(), WorldError> {
    let mut bot: Bot = (*world.get::<Bot>(id)?).clone();
    let locomotion = *world.get::<Locomotion>(id)?;
    let team = *world.get::<Team>(id)?;
    let ammo_empty = world.get::<Weapon>(id)?.state().ammo() == 0;
    let senses = PhysicsSenses::new(world.resource::<PhysicsWorld>()?);
    let target = pick_enemy(world, id, team, &locomotion, &senses)?;
    let nav = world.resource::<Waypoints>()?;
    if nav.is_empty() {
        return Err(WorldError::Downstream {
            source: Box::new(KitError::NoWaypoints),
        });
    }
    let input = BotInput {
        centre: locomotion.centre(),
        eye: locomotion.eye(),
        body: locomotion.body(),
        view: locomotion.command().view(),
        grounded: locomotion.grounded(),
        ammo_empty,
        target,
        dt,
    };
    let command = lift(
        bot.brain
            .think(&bot.config, &input, nav, &senses, &mut bot.rng),
    )?;
    *world.get_mut::<Bot>(id)? = bot;
    world.get_mut::<Locomotion>(id)?.set_command(command);
    Ok(())
}
