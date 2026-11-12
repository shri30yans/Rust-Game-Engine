//! Movement-state integration checks in a small room with a low ceiling.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
#![allow(dead_code)]

mod common;

use app::ViewAngles;
use common::{add_box, add_triangle_mesh_box, physics, run, schedule, spawn_actor, world};
use engine_physics::{ColliderId, ConvexShape, PhysicsWorld, QueryFilter, RayCast};
use engine_world::{NetId, Schedule, World};
use glam::{Vec2, Vec3};
use kit_fps::{Locomotion, MoveCommand, MoveState, MovementLog, Trigger};
use proptest::prelude::*;

const ROOF_BOTTOM_Y: f32 = 1.4;
const STANDING_HEIGHT_M: f32 = 1.8;
const CROUCHED_HEIGHT_M: f32 = 1.1;
const CAPSULE_RADIUS_M: f32 = 0.3;
const SKIN_WIDTH_M: f32 = 0.01;

fn build_room() -> Result<(World, Schedule, NetId, ColliderId), Box<dyn std::error::Error>> {
    let mut physics = physics()?;
    add_box(
        &mut physics,
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(2.0, 0.1, 2.0),
    )?;
    let roof = add_box(
        &mut physics,
        Vec3::new(0.0, ROOF_BOTTOM_Y + 0.05, 0.5),
        Vec3::new(2.0, 0.05, 1.5),
    )?;
    for (centre, half) in [
        (Vec3::new(-2.1, 1.0, 0.0), Vec3::new(0.1, 1.0, 2.0)),
        (Vec3::new(2.1, 1.0, 0.0), Vec3::new(0.1, 1.0, 2.0)),
        (Vec3::new(0.0, 1.0, -2.1), Vec3::new(2.0, 1.0, 0.1)),
        (Vec3::new(0.0, 1.0, 2.1), Vec3::new(2.0, 1.0, 0.1)),
    ] {
        add_box(&mut physics, centre, half)?;
    }
    let mut world = world(physics, 91)?;
    let centre = Vec3::new(0.0, STANDING_HEIGHT_M * 0.5, -1.5);
    let (actor, _) = spawn_actor(&mut world, centre, 0, 100)?;
    Ok((world, schedule(91)?, actor, roof))
}

fn command(forward: f32, sprint: bool, crouch: bool, jump: bool) -> MoveCommand {
    MoveCommand::new(
        Vec2::new(0.0, forward),
        ViewAngles::ZERO,
        jump,
        jump,
        Trigger::Released,
        false,
    )
    .expect("test command is valid")
    .with_movement(sprint, crouch)
}

fn crouched(state: MoveState) -> bool {
    matches!(state, MoveState::Crouch | MoveState::Slide { .. })
}

fn mode(world: &World, actor: NetId) -> Result<MoveState, Box<dyn std::error::Error>> {
    Ok(world.get::<Locomotion>(actor)?.move_state())
}

fn top_probe_hits_actor(
    physics: &PhysicsWorld,
    centre: Vec3,
    height: f32,
) -> Result<bool, Box<dyn std::error::Error>> {
    let feet_y = centre.y - height * 0.5;
    let probe_y = feet_y + CROUCHED_HEIGHT_M + (ROOF_BOTTOM_Y - CROUCHED_HEIGHT_M) * 0.5;
    let ray = RayCast::new(Vec3::new(-1.8, probe_y, centre.z), Vec3::X, 3.6, true)?;
    Ok(physics
        .ray_cast(ray, QueryFilter::default())
        .is_some_and(|hit| hit.owner.body.is_some()))
}

#[test]
fn low_roof_blocks_standing_until_clear_and_capsule_tracks_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut world, mut schedule, actor, roof) = build_room()?;
    for tick in 0..5 {
        run(&mut world, &mut schedule, tick)?;
    }
    let mut locomotion = world.get_mut::<Locomotion>(actor)?;
    locomotion.set_command(command(-1.0, false, true, false));
    drop(locomotion);
    for tick in 5..17 {
        world
            .get_mut::<Locomotion>(actor)?
            .set_command(command(-1.0, false, true, false));
        run(&mut world, &mut schedule, tick)?;
    }
    assert_eq!(mode(&world, actor)?, MoveState::Crouch);
    let centre = world.get::<Locomotion>(actor)?.centre();
    assert!(!top_probe_hits_actor(
        world.resource::<PhysicsWorld>()?,
        centre,
        CROUCHED_HEIGHT_M
    )?);

    world
        .get_mut::<Locomotion>(actor)?
        .set_command(command(0.0, false, false, false));
    run(&mut world, &mut schedule, 17)?;
    assert_eq!(
        mode(&world, actor)?,
        MoveState::Crouch,
        "a 1.4 m roof refuses standing"
    );
    assert!(!top_probe_hits_actor(
        world.resource::<PhysicsWorld>()?,
        world.get::<Locomotion>(actor)?.centre(),
        CROUCHED_HEIGHT_M,
    )?);

    world
        .resource_mut::<PhysicsWorld>()?
        .remove_collider(roof)?;
    run(&mut world, &mut schedule, 18)?;
    assert_eq!(mode(&world, actor)?, MoveState::Walk);
    let centre = world.get::<Locomotion>(actor)?.centre();
    assert!(top_probe_hits_actor(
        world.resource::<PhysicsWorld>()?,
        centre,
        STANDING_HEIGHT_M
    )?);
    let events = world.resource::<MovementLog>()?.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].from, MoveState::Crouch);
    assert_eq!(events[0].to, MoveState::Walk);
    Ok(())
}

#[test]
fn mantle_finishes_standing_on_a_clear_box_top() -> Result<(), Box<dyn std::error::Error>> {
    mantle_finishes_on_platform(add_box)
}

#[test]
fn mantle_finishes_standing_on_a_triangle_mesh_box() -> Result<(), Box<dyn std::error::Error>> {
    mantle_finishes_on_platform(add_triangle_mesh_box)
}

type AddPlatform =
    fn(&mut PhysicsWorld, Vec3, Vec3) -> Result<ColliderId, Box<dyn std::error::Error>>;

fn mantle_finishes_on_platform(
    add_platform: AddPlatform,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut physics = physics()?;
    add_platform(
        &mut physics,
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(2.0, 0.1, 2.0),
    )?;
    add_box(
        &mut physics,
        Vec3::new(0.0, 0.6, -1.0),
        Vec3::new(1.0, 0.6, 0.5),
    )?;
    let mut world = world(physics, 93)?;
    let (actor, _) = spawn_actor(&mut world, Vec3::new(0.0, 0.9, 0.0), 0, 100)?;
    let mut schedule = schedule(93)?;
    for tick in 0..5 {
        run(&mut world, &mut schedule, tick)?;
    }
    let jump = MoveCommand::new(
        Vec2::Y,
        ViewAngles::ZERO,
        true,
        true,
        Trigger::Released,
        false,
    )?
    .with_movement(false, false);
    world.get_mut::<Locomotion>(actor)?.set_command(jump);
    run(&mut world, &mut schedule, 5)?;
    assert!(matches!(mode(&world, actor)?, MoveState::Mantle { .. }));
    for tick in 6..52 {
        world
            .get_mut::<Locomotion>(actor)?
            .set_command(MoveCommand::IDLE);
        run(&mut world, &mut schedule, tick)?;
    }
    assert_eq!(mode(&world, actor)?, MoveState::Walk);
    let locomotion = world.get::<Locomotion>(actor)?;
    assert!((locomotion.centre().y - 2.1).abs() < 0.05);
    let capsule =
        ConvexShape::capsule(STANDING_HEIGHT_M * 0.5 - CAPSULE_RADIUS_M, CAPSULE_RADIUS_M)?;
    assert!(!world.resource::<PhysicsWorld>()?.intersects_shape(
        capsule,
        locomotion.centre(),
        glam::Quat::IDENTITY,
        QueryFilter::default().excluding_body(locomotion.body()),
    )?);
    Ok(())
}

proptest! {
    #[test]
    fn random_commands_keep_mode_and_shape_synchronized(
        inputs in prop::collection::vec((-1.0f32..=1.0f32, any::<bool>(), any::<bool>(), any::<bool>()), 1..80)
    ) {
        let (mut world, mut schedule, actor, _roof) = build_room()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut tick = 0_u32;
        for _ in 0..5 {
            run(&mut world, &mut schedule, tick)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            tick += 1;
        }
        let mut initial = world.get_mut::<Locomotion>(actor)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        initial.set_command(command(-1.0, false, true, false));
        drop(initial);
        for _ in 0..12 {
            run(&mut world, &mut schedule, tick)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            tick += 1;
        }
        for (forward, sprint, crouch_intent, jump) in inputs {
            let state = mode(&world, actor)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let centre = world.get::<Locomotion>(actor)
                .map_err(|error| TestCaseError::fail(error.to_string()))?.centre();
            let physics = world.resource::<PhysicsWorld>()
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let expected_height = if crouched(state) { CROUCHED_HEIGHT_M } else { STANDING_HEIGHT_M };
            prop_assert_eq!(
                top_probe_hits_actor(physics, centre, expected_height)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?,
                !crouched(state),
                "state and capsule dimensions diverged: {:?}", state,
            );
            let inset = expected_height * 0.5 - CAPSULE_RADIUS_M - SKIN_WIDTH_M;
            let inset_shape = ConvexShape::capsule(inset, CAPSULE_RADIUS_M - SKIN_WIDTH_M)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let body = world.get::<Locomotion>(actor)
                .map_err(|error| TestCaseError::fail(error.to_string()))?.body();
            prop_assert!(!physics.intersects_shape(inset_shape, centre, glam::Quat::IDENTITY, QueryFilter::default().excluding_body(body))
                .map_err(|error| TestCaseError::fail(error.to_string()))?,
                "capsule penetrated static geometry deeper than the skin width");
            let mut locomotion = world.get_mut::<Locomotion>(actor)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            locomotion.set_command(command(forward * 0.25, sprint, crouch_intent, jump));
            drop(locomotion);
            run(&mut world, &mut schedule, tick)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            tick += 1;
        }
    }
}
