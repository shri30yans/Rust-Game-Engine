//! Bots: computer-controlled actors that see, chase, aim and shoot back.
//!
//! A bot is an ordinary locomotion actor. Its brain writes a [`MoveCommand`] each tick and the
//! same locomotion and weapon systems act on it as on the player. The brain is a state machine:
//! `Idle`, then `Patrol` between waypoints, then `Chase` once it sees an enemy, then `Attack` when
//! it is within engage range. Losing sight of an enemy for more than `forget_ticks` ticks returns
//! it to `Patrol`.
//!
//! Patrol and chase move through the [`Waypoints`] link graph: when the destination is out of
//! sight the brain steers to the next waypoint on the route, so a wall between the bot and its
//! goal is walked around. A moving bot that covers less than `stuck_progress` metres in
//! `stuck_ticks` ticks is stuck: it drops its enemy and patrols from a waypoint its own RNG picks.
//! A bot's validated config can be replaced without resetting its brain, route, target, memory or
//! random stream.
//! Local obstacles (a low ledge, a wall it touches) are handled by probes in the brain itself.
//!
//! Perception goes through the [`Senses`] trait. [`PhysicsSenses`] answers with rays through the
//! physics world; tests answer with a scripted fake, so the brain is tested without physics.
//!
//! Invariants: a bot fires only while it sees its target, is aiming at it within one tick of
//! turning, and has been in sight for at least `reaction_ticks`; its aim error only shrinks while
//! it sees its target; its brain depends only on world state and its own RNG stream, so equal
//! seeds give equal runs; a bot world with bots has at least one waypoint.
//!
//! The folder splits the parts by job: `config` (tuning), `senses` (perception), `brain` (state and
//! the per-tick decision), `memory` (engagement and phase), `stuck` (progress watch), `navigation`
//! (steering goals and routes), `aim` (turning the view) and `component` (the bot and its system).

mod aim;
mod brain;
mod component;
mod config;
mod memory;
mod navigation;
mod senses;
mod stuck;

#[cfg(test)]
mod tests;

pub use brain::{BotBrain, BotInput, Phase};
pub use component::{Bot, bot_system};
pub use config::{BotConfig, BotParams};
pub use senses::{PhysicsSenses, Senses, Target};
