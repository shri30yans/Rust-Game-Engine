//! Ground probing for the character controller: slope-limit checks and snap-to-ground.
//!
//! These are read-only queries against the world. A surface steeper than the slope limit is never
//! treated as ground.

use super::config::CharacterConfig;
use crate::convert::{from_rapier_vec, to_rapier_pose, to_rapier_vec};
use crate::{PhysicsWorld, QueryFilter};
use glam::{Quat, Vec3};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::{Collider, ColliderHandle, Ray};

/// Whether a surface with this outward normal is within the character's slope limit.
fn walkable(normal: Vec3, slope_limit_radians: f32) -> bool {
    normal.y >= slope_limit_radians.cos()
}

/// Whether the surface under the character's lowest point is steeper than the slope limit.
///
/// A ray from the lowest point of the capsule, straight down, reads the surface normal. A shape
/// cast is not used here: when the capsule already rests within the skin of a face, the cast
/// reports a penetrating hit whose normal is not reliable. The ray reaches the step height plus
/// the snap distance plus the skin: after a rise the surface can be a step height below the feet.
/// `false` when no surface is in reach (the character is in the air).
pub(super) fn steep_ground_below(
    world: &PhysicsWorld,
    filter: QueryFilter,
    config: &CharacterConfig,
    centre: Vec3,
) -> bool {
    let feet = centre - Vec3::Y * (config.capsule_half_height + config.capsule_radius);
    let reach = config.step_height + config.ground_snap_distance + config.skin_width;
    let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
    let query_filter = filter.rapier_filter(&predicate);
    let queries = world.query_pipeline(query_filter);
    let ray = Ray::new(to_rapier_vec(feet), to_rapier_vec(-Vec3::Y));
    queries
        .cast_ray_and_get_normal(&ray, reach, true)
        .is_some_and(|(_, hit)| !walkable(from_rapier_vec(hit.normal), config.slope_limit_radians))
}

/// Snaps a descending character to ground when Rapier 0.36 did not (it only snaps when already
/// grounded at the move's start, using a smaller prediction distance than our public contract).
/// A surface steeper than the slope limit is never snapped to: that would mark a non-walkable face
/// as ground.
pub(super) fn ground_snap(
    world: &PhysicsWorld,
    filter: QueryFilter,
    config: &CharacterConfig,
    shape: &dyn rapier3d::prelude::Shape,
    end_position: Vec3,
) -> Option<Vec3> {
    if steep_ground_below(world, filter, config, end_position) {
        return None;
    }
    let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
    let query_filter = filter.rapier_filter(&predicate);
    let queries = world.query_pipeline(query_filter);
    let end_pose = to_rapier_pose(end_position, Quat::IDENTITY);
    let (_, hit) = queries.cast_shape(
        &end_pose,
        to_rapier_vec(-Vec3::Y),
        shape,
        ShapeCastOptions {
            target_distance: config.skin_width,
            stop_at_penetration: false,
            max_time_of_impact: config.ground_snap_distance,
            compute_impact_geometry_on_penetration: true,
        },
    )?;
    let distance = hit.time_of_impact;
    if !distance.is_finite() {
        return None;
    }
    Some(-Vec3::Y * distance)
}
