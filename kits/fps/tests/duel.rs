//! Headless duels: a bot against a stationary player, a scripted player against a stationary bot,
//! and a wall between two actors. The scenes run the same systems a game schedules.

#[allow(
    dead_code,
    reason = "each test binary uses a subset of the shared scene builders"
)]
mod common;

use std::error::Error;

use engine_physics::PhysicsWorld;
use engine_world::{NetId, World};
use glam::Vec3;
use kit_fps::{DamageLog, Weapon};

use common::{
    STANDING_CENTRE_Y, add_box, aim_and_fire, duel, health_of, physics, rifle, run, schedule,
    set_command, spawn_actor, world,
};

type TestResult = Result<(), Box<dyn Error>>;

/// Twenty simulated seconds at 60 ticks per second.
const TWENTY_SECONDS: u32 = 1200;

/// Rounds left in the magazine of an actor's weapon.
fn ammo_of(world: &World, id: NetId) -> Result<u32, Box<dyn Error>> {
    Ok(world.get::<Weapon>(id)?.state().ammo())
}

/// Runs the duel until the player dies, returning the tick of death if it happens in time.
fn run_until_player_dies(duel: &mut common::Duel) -> Result<Option<u32>, Box<dyn Error>> {
    for tick in 0..TWENTY_SECONDS {
        run(&mut duel.world, &mut duel.schedule, tick)?;
        if health_of(&duel.world, duel.player)? == 0 {
            return Ok(Some(tick));
        }
    }
    Ok(None)
}

#[test]
fn a_bot_kills_a_stationary_player_within_twenty_seconds() -> TestResult {
    let mut duel = duel(7, false)?;
    let death = run_until_player_dies(&mut duel)?;
    let tick = death.ok_or("the bot did not kill the player within twenty seconds")?;

    // The damage queue is drained within each tick, so the bot's shots are seen through the
    // player's health and the bot's ammunition rather than through the event list.
    assert!(ammo_of(&duel.world, duel.bot)? < 30, "the bot never fired");
    assert_eq!(health_of(&duel.world, duel.bot)?, 100);
    assert!(
        duel.world
            .resource::<DamageLog>()?
            .deaths()
            .contains(&duel.player)
    );
    assert!(tick > 0);
    Ok(())
}

#[test]
fn equal_seeds_give_identical_duels() -> TestResult {
    let mut first = duel(7, false)?;
    let mut second = duel(7, false)?;
    let first_death = run_until_player_dies(&mut first)?;
    let second_death = run_until_player_dies(&mut second)?;
    assert!(first_death.is_some(), "the duel ended without a death");
    assert_eq!(first_death, second_death);
    assert_eq!(first.world.state_hash()?, second.world.state_hash()?);
    Ok(())
}

#[test]
fn a_scripted_player_kills_a_stationary_bot_in_the_expected_ticks() -> TestResult {
    // Rifle damage 25 against 100 hit points: four hits. Shots are fired on ticks 0, 6, 12 and 18
    // (the fire interval), and the fourth hit kills on tick 18 itself.
    let expected_death_tick = 18;

    let mut physics: PhysicsWorld = physics()?;
    add_box(
        &mut physics,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(50.0, 0.5, 50.0),
    )?;
    let mut world = world(physics, 3)?;
    let (player, _) = spawn_actor(&mut world, Vec3::new(0.0, STANDING_CENTRE_Y, 0.0), 0, 100)?;
    let (bot, _) = spawn_actor(&mut world, Vec3::new(0.0, STANDING_CENTRE_Y, -10.0), 1, 100)?;
    world.insert(player, Weapon::new(rifle()?))?;
    let mut schedule = schedule(3)?;

    // The damage queue is drained within each tick, so hits are counted as the bot's health drops.
    let mut death = None;
    let mut hits = 0;
    let mut previous = health_of(&world, bot)?;
    for tick in 0..60 {
        set_command(&mut world, player, aim_and_fire(tick % 6 == 0)?)?;
        run(&mut world, &mut schedule, tick)?;
        let current = health_of(&world, bot)?;
        if current < previous {
            assert_eq!(
                previous - current,
                25,
                "a hit did not deal the rifle's damage"
            );
            hits += 1;
        }
        previous = current;
        if death.is_none() && current == 0 {
            death = Some(tick);
            assert!(
                world.resource::<DamageLog>()?.deaths().contains(&bot),
                "the death was not logged"
            );
        }
    }
    assert_eq!(death, Some(expected_death_tick));
    assert_eq!(hits, 4);
    assert_eq!(health_of(&world, player)?, 100);
    Ok(())
}

#[test]
fn a_wall_between_the_duellists_stops_the_bot_firing() -> TestResult {
    let mut duel = duel(11, true)?;
    for tick in 0..TWENTY_SECONDS / 2 {
        run(&mut duel.world, &mut duel.schedule, tick)?;
    }
    assert_eq!(health_of(&duel.world, duel.player)?, 100);
    assert_eq!(
        ammo_of(&duel.world, duel.bot)?,
        30,
        "the bot fired through the wall"
    );
    assert!(duel.world.resource::<DamageLog>()?.events().is_empty());
    Ok(())
}
