//! Hearing: a bot that is not fighting turns towards a shot fired within earshot.
//!
//! Every actor's shot, the player's or another bot's, is heard by the idle bots near it, so a
//! firefight draws its neighbours in. The bot learns the shooter's position, not the shooter, and
//! walks the route to it; its own sight then takes over. Shots are read from the kit's
//! [`ShotFired`] events, so hearing may run in any order among the other readers of the tick's
//! events.
//!
//! Invariants: only shots fired this tick are heard; a bot never hears its own shot; a bot that
//! already has an enemy in mind is not redirected; dead bots (no hit points left) hear nothing;
//! this system writes only bot brains.

use engine_world::{SystemContext, World, WorldError};
use kit_fps::{Bot, Health, Locomotion, ShotFired, error::lift};

use crate::data::Tuning;
use crate::rules::Actors;
use crate::sim::actor_id;

/// Alerts every idle, living bot within the tuned hearing range of a shot fired this tick.
///
/// # Errors
///
/// Returns a world error when an event log or an actor's components are missing.
pub fn hear_shots(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let actor_count = world.resource::<Actors>()?.slots().len();
    let hearing_range = world.resource::<Tuning>()?.bots.hearing_m;
    let shot_count = world.events::<ShotFired>()?.len();
    for index in 0..shot_count {
        let Some(shot) = world.events::<ShotFired>()?.get(index).copied() else {
            break;
        };
        for listener in 0..actor_count {
            let id = actor_id(world, listener)?;
            if id == shot.shooter || !world.get::<Health>(id)?.is_alive() {
                continue;
            }
            // The player has no brain: nothing to alert.
            let Ok(engaged) = world.get::<Bot>(id).map(|bot| bot.brain().is_engaged()) else {
                continue;
            };
            if engaged {
                continue;
            }
            let eye = world.get::<Locomotion>(id)?.eye();
            if (eye - shot.origin).length() > hearing_range {
                continue;
            }
            lift(world.get_mut::<Bot>(id)?.alert(shot.origin))?;
        }
    }
    Ok(())
}
