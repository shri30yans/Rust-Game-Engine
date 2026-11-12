//! Scripted headless checks of the character and the scene: fall, walk, jump, walls, the stair
//! step, the ramps and the crate stack. Each test drives `App::step` with `PlayerCommand`s and
//! asserts measured motion against a stated tolerance. No GPU, no sleeps.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{
    FACING_PLUS_X, capsule_penetrates_world, centre, crate_speeds, feet_height, hold,
    horizontal_distance, idle, idle_command, jump_once, new_app, place, state, tick, walk_forward,
};
use engine_physics::PhysicsWorld;
use glam::Vec3;
use sandbox::scene::{Bodies, BoxRole, BoxSpec, ramp, spawn_box};
use sandbox::sim::CAPSULE_RADIUS_M;

/// Jump launch speed, and gravity, as the test's own oracle for the expected apex.
const JUMP_SPEED_MPS: f32 = 5.0;
const GRAVITY_MPS2: f32 = 9.81;
/// Apex of a jump from rest: v^2 / (2 g).
const JUMP_APEX_M: f32 = JUMP_SPEED_MPS * JUMP_SPEED_MPS / (2.0 * GRAVITY_MPS2);

/// Places the character on the floor at `(x, z)` facing `yaw` and lets it settle.
fn settle_on_floor(app: &mut app::App<sandbox::SandboxGame>, x: f32, z: f32, yaw: f32) {
    place(app, Vec3::new(x, 1.0, z), yaw);
    idle(app, 30);
}

#[test]
fn falls_and_lands_on_the_floor() {
    let mut app = new_app();
    place(&mut app, Vec3::new(0.0, 3.0, 0.0), 0.0);
    idle(&mut app, 60);
    assert!(
        state(&app).grounded(),
        "character should be grounded after 60 ticks"
    );
    let feet = feet_height(&app);
    assert!(
        feet.abs() <= 0.02,
        "feet should rest within 0.02 m of the floor, got {feet}"
    );
}

#[test]
fn walks_four_metres_per_second() {
    let mut app = new_app();
    settle_on_floor(&mut app, 0.0, 0.0, FACING_PLUS_X);
    let start = centre(&app);
    hold(&mut app, 60, walk_forward());
    let distance = horizontal_distance(start, centre(&app));
    // 4.0 m/s for one second; 2 % tolerance is 0.08 m.
    assert!(
        (distance - 4.0).abs() <= 0.08,
        "one second of full forward walk should cover 4.0 m +/- 2 %, got {distance} m"
    );
    assert!(
        state(&app).grounded(),
        "walking on the floor should stay grounded"
    );
}

#[test]
fn jump_reaches_the_ballistic_apex_and_lands() {
    let mut app = new_app();
    settle_on_floor(&mut app, 0.0, 0.0, FACING_PLUS_X);
    let base = centre(&app).y;
    tick(&mut app, jump_once());
    let mut highest = base;
    for _ in 0..60 {
        tick(&mut app, idle_command());
        highest = highest.max(centre(&app).y);
    }
    let apex = highest - base;
    let error = (apex - JUMP_APEX_M).abs() / JUMP_APEX_M;
    // Expected apex v^2/(2g) = 1.274 m; tolerance 5 %.
    assert!(
        error <= 0.05,
        "jump apex should be {JUMP_APEX_M} m +/- 5 %, measured {apex} m ({:.2} % off)",
        error * 100.0
    );
    idle(&mut app, 60);
    assert!(
        state(&app).grounded(),
        "character should land and be grounded again"
    );
    assert!(
        feet_height(&app).abs() <= 0.02,
        "landed feet should be within 0.02 m of the floor"
    );
}

#[test]
fn wall_stops_the_walk_outside_the_wall() {
    let mut app = new_app();
    // The far wall's inner face is at x = 20.0; the capsule centre stops one radius short.
    settle_on_floor(&mut app, 14.0, 0.0, FACING_PLUS_X);
    for _ in 0..120 {
        tick(&mut app, walk_forward());
        assert!(
            !capsule_penetrates_world(&app),
            "character must never penetrate the scene, at x = {}",
            centre(&app).x
        );
    }
    let x = centre(&app).x;
    let expected = 20.0 - CAPSULE_RADIUS_M;
    assert!(
        (x - expected).abs() <= 0.05,
        "walk into the wall should stop at x = {expected} +/- 0.05, got {x}"
    );
}

#[test]
fn climbs_a_quarter_metre_step_while_walking() {
    let mut app = new_app();
    // The stair step is 0.25 m high and spans x in [-11, -9].
    settle_on_floor(&mut app, -12.5, 0.0, FACING_PLUS_X);
    for _ in 0..45 {
        tick(&mut app, walk_forward());
        assert!(
            !capsule_penetrates_world(&app),
            "no penetration while climbing"
        );
    }
    let x = centre(&app).x;
    assert!(
        (-11.0..=-9.0).contains(&x),
        "character should be on the step, x = {x}"
    );
    assert!(
        state(&app).grounded(),
        "character should be grounded on the step"
    );
    let feet = feet_height(&app);
    assert!(
        (feet - 0.25).abs() <= 0.03,
        "feet should rest on the step top at 0.25 m, got {feet}"
    );
}

#[test]
fn does_not_climb_a_one_metre_wall() {
    let mut app = new_app();
    let wall = BoxSpec::fixed(
        Vec3::new(-4.0, 0.5, 0.0),
        Vec3::new(0.25, 0.5, 2.0),
        BoxRole::Wall,
    );
    spawn_box(app.world_mut(), wall).expect("spawn test wall");
    settle_on_floor(&mut app, -6.0, 0.0, FACING_PLUS_X);
    for _ in 0..60 {
        tick(&mut app, walk_forward());
        assert!(
            !capsule_penetrates_world(&app),
            "no penetration at a 1 m wall"
        );
    }
    let x = centre(&app).x;
    // The wall's near face is at x = -4.25; the centre stops one radius short of it.
    assert!(
        x < -4.25,
        "character must stay on the near side of the wall, x = {x}"
    );
    let feet = feet_height(&app);
    assert!(
        feet.abs() <= 0.03,
        "character must not climb a 1 m wall, feet at {feet}"
    );
}

#[test]
fn walks_up_a_twenty_degree_ramp() {
    let mut app = new_app();
    settle_on_floor(&mut app, 0.0, -8.0, FACING_PLUS_X);
    hold(&mut app, 60, walk_forward());
    let feet = feet_height(&app);
    assert!(
        state(&app).grounded(),
        "character should be grounded on the ramp"
    );
    assert!(
        feet > 0.2,
        "walking up a 20 degree ramp should raise the feet above 0.2 m, got {feet}"
    );
}

#[test]
fn a_fifty_degree_ramp_is_too_steep_to_walk_up() {
    let mut app = new_app();
    let steep = ramp(-15.0, 15.0, 50.0_f32.to_radians());
    spawn_box(app.world_mut(), steep).expect("spawn steep ramp");
    settle_on_floor(&mut app, -17.0, 15.0, FACING_PLUS_X);
    let mut highest_feet = f32::MIN;
    for _ in 0..120 {
        tick(&mut app, walk_forward());
        assert!(
            !capsule_penetrates_world(&app),
            "no penetration at the steep ramp"
        );
        highest_feet = highest_feet.max(feet_height(&app));
    }
    let x = centre(&app).x;
    let feet = feet_height(&app);
    assert!(
        highest_feet < 0.2,
        "a 50 degree ramp is above the 45 degree limit; the feet must never rise past 0.2 m, \
         highest was {highest_feet} m (final x = {x} m, final feet = {feet} m)"
    );
}

#[test]
fn walking_into_the_crate_stack_never_penetrates() {
    let mut app = new_app();
    // The crate stack is centred at x = 12, z = 6; approach it from x = 8 along the stack's row.
    settle_on_floor(&mut app, 8.0, 6.0, FACING_PLUS_X);
    for _ in 0..120 {
        tick(&mut app, walk_forward());
        assert!(
            !capsule_penetrates_world(&app),
            "character must never overlap the world, at x = {}",
            centre(&app).x
        );
    }
    assert!(
        centre(&app).x > 10.0,
        "the walk should reach the crate stack, final x = {}",
        centre(&app).x
    );
}

#[test]
fn crate_stack_settles_without_moving_far() {
    let mut app = new_app();
    settle_on_floor(&mut app, -15.0, -15.0, FACING_PLUS_X);
    idle(&mut app, 120);
    let speeds = crate_speeds(&app);
    assert_eq!(speeds.len(), 18, "the scene should hold 18 crates");
    let fastest = speeds.iter().copied().fold(0.0_f32, f32::max);
    assert!(
        fastest < 0.05,
        "every crate should be at rest by tick 120, fastest {fastest} m/s"
    );

    let physics = app.world().resource::<PhysicsWorld>().expect("physics");
    let crates = app
        .world()
        .resource::<Bodies>()
        .expect("bodies")
        .entries()
        .iter()
        .filter(|entry| entry.role == BoxRole::Crate)
        .map(|entry| {
            physics
                .body_state(entry.body)
                .expect("crate body")
                .translation
                .y
        })
        .collect::<Vec<f32>>();
    // Two layers: bottom centres at 0.25 m, top centres at 0.77 m (0.25 + 0.52 pitch).
    for y in crates {
        let on_bottom = (y - 0.25).abs() <= 0.03;
        let on_top = (y - 0.77).abs() <= 0.03;
        assert!(
            on_bottom || on_top,
            "crate centre at y = {y} is not in a settled layer"
        );
    }
}
