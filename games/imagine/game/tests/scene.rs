//! The arena as built: waypoints and spawn points stand clear of geometry, the spawns are spread
//! and mostly out of each other's sight, the perimeter is closed, the 1.0 m ledge needs a jump
//! and the stair can be walked.

mod common;

use common::{EYE_HEIGHT_M, Motion, TestResult, line_is_clear, walk};
use engine_physics::{
    BodyBuilder, ColliderBuilder, ConvexShape, PhysicsWorld, QueryFilter, RayCast,
};
use engine_world::World;
use glam::{Quat, Vec3};
use imagine::data::{
    CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M, CENTRE_ABOVE_FEET_M, Tuning, load_tuning,
};
use imagine::scene::{ArenaData, SceneContent, start_scene_from_content};
use kit_fps::{PhysicsSenses, Senses, Waypoints};

/// Extra height, in metres, a capsule is lifted by before its overlap is tested, so that its
/// lowest point does not touch the floor it stands on.
const CONTACT_LIFT_M: f32 = 0.01;
/// The smallest distance between two spawn points, in metres.
const MIN_SPAWN_SPACING_M: f32 = 8.0;
/// Rays cast from the arena's interior to check that the perimeter is closed.
const CLOSURE_RAYS: u16 = 16;
/// The longest a closure ray may be, in metres: longer than the arena's diagonal.
const CLOSURE_RAY_MAX_M: f32 = 100.0;
/// Where the closure rays start: open ground south of the building, at eye-ish height.
const CLOSURE_ORIGIN: Vec3 = Vec3::new(0.0, 1.0, 15.0);
/// Tolerance on the world's own coordinates, in metres.
const EPS_M: f32 = 1e-3;
const ARENA_HALF_EXTENT_M: f32 = 30.0;
const WALKWAY_HEIGHT_M: f32 = 2.0;

/// A world holding only the arena's geometry, for queries that must not see any actor.
fn scene_world() -> Result<World, Box<dyn std::error::Error>> {
    let mut world = World::new(1)?;
    world.insert_resource(PhysicsWorld::new(engine_physics::PhysicsConfig::new(
        common::dt(),
        Vec3::ZERO,
    )?))?;
    let content = common::content_pack()?;
    world.insert_resource(load_tuning(&content)?)?;
    world.insert_resource(SceneContent(content))?;
    engine_scene::register_components(&mut world)?;
    start_scene_from_content(&mut world, 0)?;
    Ok(world)
}

/// Whether a standing capsule with its feet at `feet` overlaps any of the arena's geometry.
fn overlaps_geometry(
    physics: &PhysicsWorld,
    feet: Vec3,
) -> Result<bool, Box<dyn std::error::Error>> {
    let centre = feet + Vec3::Y * (CENTRE_ABOVE_FEET_M + CONTACT_LIFT_M);
    let shape = ConvexShape::capsule(CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M)?;
    Ok(physics.intersects_shape(shape, centre, Quat::IDENTITY, QueryFilter::default())?)
}

#[test]
fn every_waypoint_stands_clear_of_geometry() -> TestResult {
    let world = scene_world()?;
    let physics = world.resource::<PhysicsWorld>()?;
    for point in world.resource::<ArenaData>()?.waypoints() {
        assert!(
            !overlaps_geometry(physics, *point)?,
            "waypoint {point:?} is inside geometry"
        );
    }
    Ok(())
}

#[test]
fn the_waypoint_graph_is_connected_through_clear_links() -> TestResult {
    // Building fails with a Disconnected error naming any waypoint that cannot be reached.
    let world = scene_world()?;
    let senses = PhysicsSenses::new(world.resource::<PhysicsWorld>()?);
    let waypoints = world.resource::<ArenaData>()?.waypoints().to_vec();
    let rules = world.resource::<Tuning>()?.link_rules()?;
    Waypoints::build(waypoints, &senses, &rules)?;
    Ok(())
}

#[test]
fn every_spawn_point_has_a_clear_sight_line_to_a_waypoint() -> TestResult {
    let world = scene_world()?;
    let senses = PhysicsSenses::new(world.resource::<PhysicsWorld>()?);
    let arena = world.resource::<ArenaData>()?;
    let knee = world.resource::<Tuning>()?.link_rules()?.knee_height() * Vec3::Y;
    for spawn in arena.player_spawns().iter().chain(arena.bot_spawns()) {
        let mut linked = false;
        for waypoint in arena.waypoints() {
            if senses.clear(None, *spawn + knee, *waypoint + knee)? {
                linked = true;
                break;
            }
        }
        assert!(
            linked,
            "no waypoint is in clear sight of the spawn {spawn:?}"
        );
    }
    Ok(())
}

#[test]
fn every_spawn_point_stands_clear_and_the_spawns_are_spread_apart() -> TestResult {
    let world = scene_world()?;
    let physics = world.resource::<PhysicsWorld>()?;
    let arena = world.resource::<ArenaData>()?;
    let spawns: Vec<Vec3> = arena
        .player_spawns()
        .iter()
        .chain(arena.bot_spawns().iter())
        .copied()
        .collect();
    for point in &spawns {
        assert!(
            !overlaps_geometry(physics, *point)?,
            "spawn {point:?} is inside geometry"
        );
    }
    for (index, first) in spawns.iter().enumerate() {
        for second in spawns.iter().skip(index + 1) {
            let distance = first.distance(*second);
            assert!(
                distance >= MIN_SPAWN_SPACING_M,
                "spawns {first:?} and {second:?} are {distance} m apart"
            );
        }
    }
    Ok(())
}

#[test]
fn line_of_sight_between_spawns_is_limited_to_half_the_pairs() -> TestResult {
    let world = scene_world()?;
    let physics = world.resource::<PhysicsWorld>()?;
    let arena = world.resource::<ArenaData>()?;
    let spawns: Vec<Vec3> = arena
        .player_spawns()
        .iter()
        .chain(arena.bot_spawns().iter())
        .copied()
        .collect();
    let eye = |feet: Vec3| feet + Vec3::Y * EYE_HEIGHT_M;
    let mut pairs = 0_u32;
    let mut visible = 0_u32;
    for (index, first) in spawns.iter().enumerate() {
        for second in spawns.iter().skip(index + 1) {
            pairs += 1;
            if line_is_clear(physics, eye(*first), eye(*second))? {
                visible += 1;
            }
        }
    }
    assert!(
        visible * 2 <= pairs,
        "{visible} of {pairs} spawn pairs see each other; at most half may"
    );
    Ok(())
}

#[test]
fn the_arena_is_closed_by_its_walls() -> TestResult {
    let world = scene_world()?;
    let physics = world.resource::<PhysicsWorld>()?;
    let limit = ARENA_HALF_EXTENT_M + EPS_M;
    for ray in 0..CLOSURE_RAYS {
        let angle = std::f32::consts::TAU * f32::from(ray) / f32::from(CLOSURE_RAYS);
        let direction = Vec3::new(angle.sin(), 0.0, angle.cos());
        let request = RayCast::new(CLOSURE_ORIGIN, direction, CLOSURE_RAY_MAX_M, true)?;
        let hit = physics
            .ray_cast(request, QueryFilter::default())
            .ok_or("a ray left the arena without meeting a wall")?;
        let point = hit.point;
        assert!(
            point.x.abs() <= limit && point.z.abs() <= limit,
            "ray {ray} escaped the arena at {point:?}"
        );
    }
    Ok(())
}

#[test]
fn the_platform_vertical_face_cannot_be_walked_up() -> TestResult {
    // Approach the platform's south face, away from the ramp, and hold forward for two seconds.
    let mut motion = Motion::new(Vec3::new(21.0, 0.0, -8.0))?;
    let facing_north = std::f32::consts::PI;
    for _ in 0..120 {
        motion.step(walk(facing_north, 1.0, false, false)?)?;
    }
    let centre = motion.centre()?;
    assert!(
        motion.feet_height()?.abs() < 0.05,
        "climbed the vertical face without a jump or ramp: {centre:?}"
    );
    assert!(
        centre.z < -6.0,
        "walked through the platform's vertical face: {centre:?}"
    );
    Ok(())
}

fn mantle_to_platform(feet: Vec3, yaw: f32) -> Result<Vec3, Box<dyn std::error::Error>> {
    let mut motion = Motion::new(feet)?;
    motion.step(walk(yaw, 1.0, true, true)?)?;
    for _ in 0..60 {
        motion.step(walk(yaw, 1.0, false, true)?)?;
    }
    motion.centre()
}

#[test]
fn the_east_platform_can_be_mantled_from_the_ground() -> TestResult {
    let centre = mantle_to_platform(Vec3::new(21.0, 0.0, -6.45), std::f32::consts::PI)?;
    assert!(
        centre.z > -6.0,
        "mantle did not clear the platform lip: {centre:?}"
    );
    assert!(
        centre.y - CENTRE_ABOVE_FEET_M > 1.4,
        "not on the platform: {centre:?}"
    );
    Ok(())
}

#[test]
fn the_west_platform_can_be_mantled_from_the_ground() -> TestResult {
    let centre = mantle_to_platform(Vec3::new(-16.15, 0.0, 0.0), std::f32::consts::FRAC_PI_2)?;
    assert!(
        centre.x < -16.0,
        "mantle did not clear the platform lip: {centre:?}"
    );
    assert!(
        centre.y - CENTRE_ABOVE_FEET_M > 1.4,
        "not on the platform: {centre:?}"
    );
    Ok(())
}

#[test]
fn the_stair_is_walkable_up_to_the_walkway() -> TestResult {
    // Start north of the stair's lowest step and walk south (yaw pi faces +Z) for four seconds.
    let mut motion = Motion::new(Vec3::new(0.0, 0.0, -15.0))?;
    let facing_south = std::f32::consts::PI;
    for _ in 0..240 {
        motion.step(walk(facing_south, 1.0, false, false)?)?;
    }
    let centre = motion.centre()?;
    assert!(centre.z > -9.0, "did not reach the walkway: {centre:?}");
    assert!(
        (motion.feet_height()? - WALKWAY_HEIGHT_M).abs() < 0.05,
        "not standing on the walkway: feet at {}",
        motion.feet_height()?
    );
    Ok(())
}

#[test]
fn character_movement_stops_at_another_standing_character() -> TestResult {
    let mut motion = Motion::new(Vec3::ZERO)?;
    let physics = motion.world.resource_mut::<PhysicsWorld>()?;
    for x in [-0.52, 0.52] {
        physics.insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(0.1, 1.0, 1.25))?)
                .translation(Vec3::new(x, 1.0, -1.8))?,
        )?;
    }
    let blocker_centre = Vec3::new(0.0, CENTRE_ABOVE_FEET_M, -2.0);
    {
        let physics = motion.world.resource_mut::<PhysicsWorld>()?;
        let body = physics.insert_body(BodyBuilder::kinematic().translation(blocker_centre)?);
        let shape = ConvexShape::capsule(CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M)?;
        physics.insert_collider(
            Some(body),
            ColliderBuilder::new(shape).layers(imagine::collision::character_layers()),
        )?;
    }
    let ray = RayCast::new(
        blocker_centre + Vec3::Y * 0.2 + Vec3::Z,
        -Vec3::Z,
        2.0,
        true,
    )?;
    assert!(
        motion
            .world
            .resource::<PhysicsWorld>()?
            .ray_cast(ray, imagine::collision::character_query())
            .is_some(),
        "character query did not include the standing character collider"
    );

    for _ in 0..90 {
        motion.step(walk(0.0, 1.0, false, false)?)?;
    }

    let centre = motion.centre()?;
    assert!(
        centre.z > blocker_centre.z + 0.4,
        "character passed through the standing character: {centre:?}"
    );
    Ok(())
}
