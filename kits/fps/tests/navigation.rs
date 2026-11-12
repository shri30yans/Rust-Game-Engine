//! Physics-backed navigation. The waypoint graph must respect real geometry: no link passes through
//! a wall. A bot behind an L-shaped wall must route round it to a player it cannot see.

#[allow(
    dead_code,
    reason = "each test binary uses a subset of the shared scene builders"
)]
mod common;

use std::error::Error;

use common::{
    EYE_ABOVE_CENTRE, STANDING_CENTRE_Y, add_box, bot_params, centre_of, physics, rifle, run,
    schedule, spawn_actor, waypoints, world,
};
use engine_core::SimRng;
use engine_physics::PhysicsWorld;
use glam::Vec3;
use kit_fps::{Bot, BotConfig, Weapon};

/// Ticks the bot is given to walk round the wall: 20 seconds at 60 ticks per second.
const REACH_TICKS: u32 = 1200;

/// Waypoints for the L-shaped wall scene. Index 9 is the player's position at the origin.
fn points() -> Vec<Vec3> {
    vec![
        Vec3::new(-10.0, 0.0, -10.0),
        Vec3::new(-10.0, 0.0, -5.0),
        Vec3::new(-10.0, 0.0, 0.0),
        Vec3::new(-4.0, 0.0, -10.0),
        Vec3::new(0.0, 0.0, -10.0),
        Vec3::new(7.0, 0.0, -10.0),
        Vec3::new(7.0, 0.0, -3.0),
        Vec3::new(2.0, 0.0, -3.0),
        Vec3::new(-4.0, 0.0, -2.0),
        Vec3::new(0.0, 0.0, 0.0),
    ]
}

/// The floor and an L-shaped wall: a long wall along x at z = -5 from x = -8 to 4, and a short leg
/// at its east end running from z = -5 to -1. The bot starts behind the long wall, and the player
/// stands at the origin on the other side.
fn l_wall_scene() -> Result<PhysicsWorld, Box<dyn Error>> {
    let mut physics = physics()?;
    add_box(
        &mut physics,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(50.0, 0.5, 50.0),
    )?;
    add_box(
        &mut physics,
        Vec3::new(-2.0, 1.5, -5.0),
        Vec3::new(6.0, 1.5, 0.3),
    )?;
    add_box(
        &mut physics,
        Vec3::new(4.0, 1.5, -3.0),
        Vec3::new(0.3, 1.5, 2.0),
    )?;
    Ok(physics)
}

#[test]
fn no_link_passes_through_the_wall() -> Result<(), Box<dyn Error>> {
    let physics = l_wall_scene()?;
    let nav = waypoints(&physics, points())?;
    let id = |index: usize| nav.id(index).ok_or("waypoint index out of range");
    // Waypoint 3 is behind the long wall and waypoint 9 is the player's side of it; the direct
    // segment between them crosses the wall, so the route must not be a direct hop.
    assert_ne!(nav.next_hop(id(3)?, id(9)?), Some(id(9)?));
    // Waypoint 4 is behind the wall at x = 0 and waypoint 7 is in front of it.
    assert_ne!(nav.next_hop(id(4)?, id(7)?), Some(id(7)?));
    Ok(())
}

#[test]
fn a_bot_behind_an_l_shaped_wall_routes_round_it_to_a_player_it_cannot_see()
-> Result<(), Box<dyn Error>> {
    let physics = l_wall_scene()?;
    let nav = waypoints(&physics, points())?;
    let mut world = world(physics, 21)?;
    // The player is durable so the bot's fire cannot end the scene before it arrives.
    let (player, _) = spawn_actor(
        &mut world,
        Vec3::new(0.0, STANDING_CENTRE_Y, 0.0),
        0,
        100_000,
    )?;
    let (bot, _) = spawn_actor(
        &mut world,
        Vec3::new(-4.0, STANDING_CENTRE_Y, -10.0),
        1,
        100,
    )?;
    world.insert(player, Weapon::new(rifle()?))?;
    world.insert(bot, Weapon::new(rifle()?))?;
    // Forgetting is set beyond the whole walk, so the bot keeps its enemy while it routes.
    let config = BotConfig::new(bot_params(REACH_TICKS))?;
    world.insert(bot, Bot::new(config, SimRng::new(21, 1)))?;
    world.insert_resource(nav)?;
    // The bot is told where the player is; the wall hides the player from the bot's sight.
    let player_eye = Vec3::new(0.0, STANDING_CENTRE_Y + EYE_ABOVE_CENTRE, 0.0);
    world.get_mut::<Bot>(bot)?.alert(player_eye)?;

    let mut schedule = schedule(21)?;
    let mut reached = None;
    for tick in 1..=REACH_TICKS {
        run(&mut world, &mut schedule, tick)?;
        let centre = centre_of(&world, bot)?;
        if Vec3::new(centre.x, 0.0, centre.z).length() <= 8.0 {
            reached = Some((tick, centre));
            break;
        }
    }
    let (tick, centre) =
        reached.ok_or("the bot never came within 8 m of the player in 20 seconds")?;
    assert!(
        centre.z > -4.5,
        "the bot was still behind the wall at tick {tick}, at {centre}"
    );
    Ok(())
}
