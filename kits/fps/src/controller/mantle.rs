//! Mantle detection: [`detect_mantle`] probes forward for a wall and a ledge the character can
//! climb onto, and returns the standing position on that ledge when one is found.
//!
//! Invariants: the wall probe starts outside the character's own capsule; a ledge is accepted only
//! when its height is inside the mantle range, the standing capsule fits there, and its surface is
//! flat enough to stand on (within the slope limit).

use engine_physics::{ConvexShape, PhysicsWorld, RayCast, ShapeCast};
use engine_world::WorldError;
use glam::{Quat, Vec3};

use super::config::ControllerConfig;
use super::locomotion::Locomotion;
use super::motor::SKIN_WIDTH_M;
use crate::error::lift;
use crate::movement::MoveState;

/// Finds a mantle destination with a forward probe, a standing capsule cast, and a normal check.
pub(super) fn detect_mantle(
    physics: &PhysicsWorld,
    locomotion: &Locomotion,
    config: &ControllerConfig,
    rotation: Quat,
    capsule_radius: f32,
) -> Result<Option<Vec3>, WorldError> {
    let yaw = rotation.to_euler(glam::EulerRot::YXZ).0;
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let crouched = matches!(
        locomotion.move_state,
        MoveState::Crouch | MoveState::Slide { .. }
    );
    let feet = locomotion.centre
        - Vec3::Y
            * if crouched {
                config.crouch_height * 0.5
            } else {
                config.stand_height * 0.5
            };
    let chest = feet
        + Vec3::Y * (config.stand_height * 0.4)
        + forward * (capsule_radius + CAPSULE_PROBE_RADIUS_M - SKIN_WIDTH_M * 2.0);
    let probe = lift(ConvexShape::ball(CAPSULE_PROBE_RADIUS_M))?;
    let wall_cast = lift(ShapeCast::new(
        probe,
        chest,
        Quat::IDENTITY,
        forward,
        config.mantle_reach,
    ))?;
    let Some(wall_hit) = physics.shape_cast(wall_cast, config.query_filter) else {
        return Ok(None);
    };
    let beyond_wall = chest + forward * (wall_hit.distance + 2.0 * capsule_radius + SKIN_WIDTH_M);
    let start = Vec3::new(
        beyond_wall.x,
        feet.y + config.mantle_max_height + config.stand_height * 0.5 + SKIN_WIDTH_M,
        beyond_wall.z,
    );
    let standing = lift(ConvexShape::capsule(
        config.stand_height * 0.5 - capsule_radius,
        capsule_radius,
    ))?;
    let feet_y = feet.y;
    let standing_at_start = Vec3::new(
        locomotion.centre.x,
        feet_y + config.stand_height * 0.5,
        locomotion.centre.z,
    );
    if lift(physics.intersects_shape(
        standing.clone(),
        standing_at_start,
        rotation,
        config.query_filter.excluding_body(locomotion.body),
    ))? {
        return Ok(None);
    }
    let down_distance = config.mantle_max_height - config.mantle_min_height + SKIN_WIDTH_M * 2.0;
    let down_cast = lift(ShapeCast::new(
        standing.clone(),
        start,
        Quat::IDENTITY,
        -Vec3::Y,
        down_distance,
    ))?;
    let Some(top_hit) = physics.shape_cast(down_cast, config.query_filter) else {
        return Ok(None);
    };
    let target = start - Vec3::Y * top_hit.distance + Vec3::Y * (SKIN_WIDTH_M * 2.0);
    let height = target.y - config.stand_height * 0.5 - feet.y - SKIN_WIDTH_M * 2.0;
    if height < config.mantle_min_height - SKIN_WIDTH_M
        || height > config.mantle_max_height + SKIN_WIDTH_M
    {
        return Ok(None);
    }
    if lift(physics.intersects_shape(
        standing.clone(),
        target,
        rotation,
        config.query_filter.excluding_body(locomotion.body),
    ))? {
        return Ok(None);
    }
    let surface_ray = lift(RayCast::new(
        target - Vec3::Y * (config.stand_height * 0.5 - SKIN_WIDTH_M),
        -Vec3::Y,
        SKIN_WIDTH_M * 5.0,
        true,
    ))?;
    let Some(surface) = physics.ray_cast(
        surface_ray,
        config.query_filter.excluding_body(locomotion.body),
    ) else {
        return Ok(None);
    };
    if surface.owner.collider != top_hit.owner.collider
        || surface.normal.y < config.slope_limit.cos()
    {
        return Ok(None);
    }
    Ok(Some(target))
}

/// Sphere used to check for a wall without beginning the cast inside the character's capsule.
const CAPSULE_PROBE_RADIUS_M: f32 = 0.1;
