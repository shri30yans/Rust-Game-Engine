//! The schedule-facing entry points: the systems that clear the movement log and move every
//! locomotion entity, and [`move_one`], the single-character prediction entry point.
//!
//! Invariants: [`locomotion_system`] runs after bots and before physics; [`move_one`] refuses a
//! delta that is not finite and positive, so a bad frame time cannot move a character.

use engine_world::{NetId, SystemContext, World, WorldError};

use super::drive::drive;
use super::locomotion::Locomotion;
use crate::movement::MovementLog;
use crate::roster::for_each_with;

/// Moves every locomotion entity by one fixed tick and writes its transform.
///
/// Order in a schedule: after the bot system (bots set their commands first) and before the
/// physics step (the step applies the kinematic pose this system sets).
///
/// # Errors
///
/// Returns a world error when a locomotion entity lacks its body or transform, the motor or
/// physics world is missing, or physics rejects a pose.
pub fn locomotion_system(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let dt = ctx.dt;
    for_each_with::<Locomotion, _>(world, |world, id| drive(world, id, dt))
}

/// Clears movement events before bots and characters produce this tick's events.
pub fn clear_movement_log_system(
    world: &mut World,
    _ctx: &mut SystemContext<'_>,
) -> Result<(), WorldError> {
    world.resource_mut::<MovementLog>()?.clear();
    Ok(())
}

/// Advances one character with the same movement step used by [`locomotion_system`].
///
/// This is the prediction entry point for games that run one locally owned pawn.
pub fn move_one(world: &mut World, id: NetId, dt: f32) -> Result<(), WorldError> {
    if !(dt.is_finite() && dt > 0.0) {
        return Err(WorldError::Invariant(
            "movement delta must be finite and positive",
        ));
    }
    drive(world, id, dt)
}
