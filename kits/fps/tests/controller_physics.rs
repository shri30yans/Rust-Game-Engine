//! Physics-backed locomotion: a capsule walks at a 1 m wall with a 1.5 m box behind it. A held
//! jump clears both; a tap, which peaks below the wall top, does not.

#[allow(
    dead_code,
    reason = "each test binary uses a subset of the shared scene builders"
)]
mod common;

use std::error::Error;

use glam::{Vec2, Vec3};

use common::{
    STANDING_CENTRE_Y, add_box, centre_of, grounded, physics, run, schedule, set_command,
    spawn_actor, walk, world,
};

type TestResult = Result<(), Box<dyn Error>>;

/// Ticks spent standing on the floor before walking, so the capsule settles first.
const SETTLE_TICKS: u32 = 30;
/// Ticks spent walking after the settle.
const WALK_TICKS: u32 = 240;
/// Ticks the jump button is held in the held-jump run (about 1.5 s, past the landing).
const HELD_TICKS: u32 = 90;
/// Height of the wall and the box, in metres.
const WALL_TOP_M: f32 = 1.0;
const BOX_TOP_M: f32 = 1.5;

/// What one run of the scene observed.
struct Outcome {
    /// Highest feet height reached during the walk, in metres.
    max_feet: f32,
    /// Capsule centre after the run.
    centre: Vec3,
    /// Whether the capsule stood on the ground after the run.
    grounded: bool,
}

/// Walks the capsule at the obstacles, jumping on the first walking tick. The jump button is held
/// for [`HELD_TICKS`] when `held` is true, and released on the first tick when it is false.
fn walk_and_jump(held: bool) -> Result<Outcome, Box<dyn Error>> {
    let mut physics = physics()?;
    // Floor: top at y = 0.
    add_box(
        &mut physics,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(50.0, 0.5, 50.0),
    )?;
    // The 1 m wall, 2.3 to 2.5 m ahead.
    add_box(
        &mut physics,
        Vec3::new(0.0, WALL_TOP_M / 2.0, -2.4),
        Vec3::new(4.0, WALL_TOP_M / 2.0, 0.1),
    )?;
    // The 1.5 m box, just behind the wall: 3.5 to 4.5 m ahead.
    add_box(
        &mut physics,
        Vec3::new(0.0, BOX_TOP_M / 2.0, -4.0),
        Vec3::new(4.0, BOX_TOP_M / 2.0, 0.5),
    )?;
    let mut world = world(physics, 1)?;
    let (hero, _) = spawn_actor(&mut world, Vec3::new(0.0, STANDING_CENTRE_Y, 0.0), 0, 100)?;
    let mut schedule = schedule(1)?;

    let feet_offset = STANDING_CENTRE_Y;
    let mut max_feet = f32::NEG_INFINITY;
    for tick in 0..SETTLE_TICKS + WALK_TICKS {
        let command = if tick < SETTLE_TICKS {
            walk(Vec2::ZERO, false, false)?
        } else {
            let walking = tick - SETTLE_TICKS;
            let jump_held = held && walking < HELD_TICKS;
            walk(Vec2::new(0.0, 1.0), walking == 0, jump_held)?
        };
        set_command(&mut world, hero, command)?;
        run(&mut world, &mut schedule, tick)?;
        if tick >= SETTLE_TICKS {
            max_feet = max_feet.max(centre_of(&world, hero)?.y - feet_offset);
        }
    }
    Ok(Outcome {
        max_feet,
        centre: centre_of(&world, hero)?,
        grounded: grounded(&world, hero)?,
    })
}

#[test]
fn a_held_jump_clears_the_wall_and_the_box() -> TestResult {
    let outcome = walk_and_jump(true)?;
    assert!(
        outcome.max_feet > BOX_TOP_M,
        "feet peaked at {} m, below the {BOX_TOP_M} m box (centre {:?}, grounded {})",
        outcome.max_feet,
        outcome.centre,
        outcome.grounded
    );
    // Past the box's far face (z = -4.5) and the capsule radius.
    assert!(
        outcome.centre.z < -4.8,
        "capsule stopped at z = {}",
        outcome.centre.z
    );
    assert!(outcome.grounded, "capsule did not land");
    Ok(())
}

#[test]
fn a_tap_jump_does_not_clear_the_wall() -> TestResult {
    let outcome = walk_and_jump(false)?;
    assert!(
        outcome.max_feet < WALL_TOP_M,
        "feet peaked at {} m, above the {WALL_TOP_M} m wall",
        outcome.max_feet
    );
    // Still in front of the wall's near face (z = -2.3), so the wall stopped it.
    assert!(
        outcome.centre.z > -2.5,
        "capsule passed the wall at z = {}",
        outcome.centre.z
    );
    assert!(outcome.grounded);
    Ok(())
}
