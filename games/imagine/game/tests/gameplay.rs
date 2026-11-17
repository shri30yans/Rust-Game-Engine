//! Scripted, headless gameplay on the full arena with a fixed seed: a kill and a bot respawn, a
//! bot killing the player and the player's respawn, a downed player that cannot move, and a
//! minute of six bots that stays within bounds and hashes deterministically.

mod common;

use app::ViewAngles;
use common::{
    Arena, TestResult, actor_count, aim_pitch, aim_yaw, centre_of, command, eye_of, health_of,
};
use engine_world::World;
use glam::{Vec2, Vec3};
use imagine::data::{TICK_RATE_HZ, Tuning, ms_to_ticks};
use imagine::rules::{PLAYER_SLOT, Score, farthest_spawn};
use imagine::scene::ArenaData;
use imagine::sim::is_alive;
use kit_fps::{CombatStats, Locomotion, MoveCommand, PhysicsSenses, Senses, Trigger};

/// The seed every scripted scenario runs on.
const SEED: u64 = 7;
/// Ticks in one second, as a count.
fn ticks_per_second() -> u32 {
    u32::from(TICK_RATE_HZ)
}

/// The longest a scripted fight may last before the test gives up: one minute.
fn fight_limit_ticks() -> u32 {
    60 * ticks_per_second()
}

/// Ticks a delay in milliseconds lasts, or an error when it overflows.
fn ticks_for(ms: u32) -> Result<u32, &'static str> {
    ms_to_ticks(ms).ok_or("delay overflows the tick count")
}

/// The number of kills the player has scored.
fn kills(world: &World) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(world.resource::<Score>()?.kills())
}

/// The number of times the player has died.
fn deaths(world: &World) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(world.resource::<Score>()?.deaths())
}

/// The slot of the living bot nearest to `from`, if any.
fn nearest_living_bot(
    world: &World,
    from: Vec3,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    let mut best: Option<(f32, usize)> = None;
    for slot in 1..actor_count(world)? {
        if !is_alive(world, slot)? {
            continue;
        }
        let distance = centre_of(world, slot)?.distance(from);
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, slot));
        }
    }
    Ok(best.map(|(_, slot)| slot))
}

/// The nearest living bot whose body is visible from the player's eyes.
fn nearest_visible_bot(
    world: &World,
    from: Vec3,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    let player = imagine::sim::actor_id(world, PLAYER_SLOT)?;
    let player_body = world.get::<Locomotion>(player)?.body();
    let senses = PhysicsSenses::new(world.resource::<engine_physics::PhysicsWorld>()?);
    let mut best: Option<(f32, usize)> = None;
    for slot in 1..actor_count(world)? {
        if !is_alive(world, slot)? {
            continue;
        }
        let id = imagine::sim::actor_id(world, slot)?;
        let locomotion = world.get::<Locomotion>(id)?;
        let target = centre_of(world, slot)? + Vec3::Y * imagine::data::EYE_ABOVE_CENTRE_M;
        if !senses.can_see(player_body, from, locomotion.body(), target)? {
            continue;
        }
        let distance = target.distance(from);
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, slot));
        }
    }
    Ok(best.map(|(_, slot)| slot))
}

#[test]
fn the_player_kills_a_bot_and_the_bot_respawns_at_its_spawn_rule() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    let mut ticks = 0_u32;
    // The bots fight each other too, so the player shoots the nearest bot still standing.
    let mut victim = 1;
    while kills(&arena.world)? == 0 {
        if ticks >= fight_limit_ticks() {
            return Err("the player did not kill the bot within a minute".into());
        }
        let eye = eye_of(&arena.world, PLAYER_SLOT)?;
        victim = nearest_visible_bot(&arena.world, eye)?
            .or(nearest_living_bot(&arena.world, eye)?)
            .unwrap_or(victim);
        let target = centre_of(&arena.world, victim)?;
        arena.command_player(command(
            0.0,
            aim_yaw(eye, target),
            aim_pitch(eye, target),
            true,
        )?)?;
        arena.step()?;
        ticks += 1;
    }
    assert_eq!(kills(&arena.world)?, 1, "one kill is scored");
    assert!(!is_alive(&arena.world, victim)?, "the bot is down");

    // The player stops firing; the bot waits its full delay, then returns at the spawn point
    // farthest from the player.
    let respawn = ticks_for(arena.world.resource::<Tuning>()?.respawn.bot_ms)?;
    arena.command_player(command(0.0, 0.0, 0.0, false)?)?;
    arena.run(respawn - 1)?;
    assert!(
        !is_alive(&arena.world, victim)?,
        "still down one tick early"
    );
    arena.step()?;
    assert!(is_alive(&arena.world, victim)?, "back up after the delay");

    let player = centre_of(&arena.world, PLAYER_SLOT)?;
    let bot_spawns = arena.world.resource::<ArenaData>()?.bot_spawns();
    let mut threats = vec![player];
    for slot in 1..actor_count(&arena.world)? {
        if slot != victim && is_alive(&arena.world, slot)? {
            threats.push(centre_of(&arena.world, slot)?);
        }
    }
    let index = farthest_spawn(bot_spawns, threats.into_iter());
    let expected = *bot_spawns
        .get(index)
        .ok_or("the farthest spawn index is out of range")?;
    let bot = centre_of(&arena.world, victim)?;
    let feet = bot - Vec3::Y * imagine::data::CENTRE_ABOVE_FEET_M;
    assert!(
        feet.distance(expected) < 1e-3,
        "respawned at {feet:?}, expected {expected:?}"
    );
    assert_eq!(kills(&arena.world)?, 1, "the respawn does not score again");
    Ok(())
}

/// Runs the player standing still until a bot kills it. Returns the ticks it took.
fn run_until_player_down(arena: &mut Arena) -> Result<u32, Box<dyn std::error::Error>> {
    let mut ticks = 0_u32;
    while is_alive(&arena.world, PLAYER_SLOT)? {
        if ticks >= fight_limit_ticks() {
            return Err("no bot killed the player within a minute".into());
        }
        arena.command_player(MoveCommand::IDLE)?;
        arena.step()?;
        ticks += 1;
    }
    Ok(ticks)
}

#[test]
fn a_bot_with_line_of_sight_kills_the_player_and_the_player_respawns() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    run_until_player_down(&mut arena)?;
    assert_eq!(deaths(&arena.world)?, 1, "one death is scored");
    assert_eq!(kills(&arena.world)?, 0, "the player scored no kill");

    let respawn = ticks_for(arena.world.resource::<Tuning>()?.respawn.player_ms)?;
    arena.command_player(MoveCommand::IDLE)?;
    arena.run(respawn - 1)?;
    assert!(
        !is_alive(&arena.world, PLAYER_SLOT)?,
        "still down one tick early"
    );
    arena.step()?;
    assert!(
        is_alive(&arena.world, PLAYER_SLOT)?,
        "back up after the delay"
    );
    assert_eq!(
        deaths(&arena.world)?,
        1,
        "the respawn does not score a death"
    );
    let (current, max) = health_of(&arena.world, PLAYER_SLOT)?;
    assert_eq!(current, max, "the respawn restores full health");
    Ok(())
}

#[test]
fn a_downed_player_cannot_move() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    run_until_player_down(&mut arena)?;
    let before = centre_of(&arena.world, PLAYER_SLOT)?;

    // Full forward input and fire for half a second: still less than the respawn delay.
    for _ in 0..(ticks_per_second() / 2) {
        arena.command_player(command(1.0, 0.0, 0.0, true)?)?;
        arena.step()?;
    }
    assert!(!is_alive(&arena.world, PLAYER_SLOT)?, "still down");
    let after = centre_of(&arena.world, PLAYER_SLOT)?;
    assert!(
        after.distance(before) < 1e-4,
        "a downed player moved from {before:?} to {after:?}"
    );
    Ok(())
}

/// Runs one minute with six bots and the player firing straight ahead. Checks health after every
/// tick, and returns the world's state hash at the end.
fn run_one_minute(seed: u64) -> Result<u64, Box<dyn std::error::Error>> {
    let mut arena = Arena::new(seed)?;
    for _ in 0..(60 * ticks_per_second()) {
        arena.command_player(command(0.0, 0.0, 0.0, true)?)?;
        arena.step()?;
        for slot in 0..actor_count(&arena.world)? {
            let (current, max) = health_of(&arena.world, slot)?;
            assert!(current <= max, "slot {slot} has {current} of {max} health");
        }
    }
    Ok(arena.world.state_hash()?)
}

#[test]
fn six_bots_for_a_minute_stay_in_bounds_and_hash_deterministically() -> TestResult {
    let first = run_one_minute(SEED)?;
    let second = run_one_minute(SEED)?;
    let other = run_one_minute(SEED + 1)?;
    assert_eq!(first, second, "the same seed gives the same state");
    assert_ne!(first, other, "another seed gives another state");
    Ok(())
}

#[test]
fn six_bot_combat_metrics_report_engagements_after_one_minute() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    arena.command_player(MoveCommand::IDLE)?;
    arena.run(60 * ticks_per_second())?;
    let metrics = arena.world.resource::<CombatStats>()?;
    let median = metrics
        .median_engagement_distance_m()
        .ok_or("the match recorded no hit distances")?;
    let sighting_distance = metrics
        .mean_first_sighting_distance_m()
        .ok_or("the match recorded no first-sighting distances")?;
    let first_hit_distance = metrics
        .mean_first_hit_distance_m()
        .ok_or("the match recorded no first-hit distances")?;
    let sighting_to_hit = metrics
        .mean_sighting_to_hit_ticks()
        .ok_or("the match recorded no sighting-to-hit times")?;
    let engage_to_hit = metrics
        .mean_engage_to_hit_ticks()
        .ok_or("the match recorded no engage-to-hit times")?;
    println!(
        "combat metrics: shots={}, hits={}, hit_percentage={:.1}%, median_engagement_m={median:.1}, mean_first_sighting_m={sighting_distance:.1}, mean_first_hit_m={first_hit_distance:.1}, mean_sighting_to_hit_ticks={sighting_to_hit:.1}, mean_engage_to_hit_ticks={engage_to_hit:.1}, mean_time_to_kill_ticks={:?}, deaths_before_reacting={}",
        metrics.shots_fired(),
        metrics.hits(),
        metrics.hit_percentage(),
        metrics.mean_time_to_kill_ticks(),
        metrics.deaths_before_reacting(),
    );
    assert!((8.0..=25.0).contains(&median));
    assert!((8.0..=35.0).contains(&sighting_distance));
    assert!((8.0..=25.0).contains(&first_hit_distance));
    assert!(sighting_to_hit < 60.0, "sighting latency {sighting_to_hit}");
    assert!(engage_to_hit < 60.0, "engage latency {engage_to_hit}");
    Ok(())
}

fn scripted_movement_hashes(seed: u64) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let mut arena = Arena::new(seed)?;
    let mut hashes = Vec::with_capacity(600);
    for tick in 0..600_u32 {
        let phase = tick % 180;
        let forward = if phase < 110 { 1.0 } else { 0.0 };
        let sprint = phase < 80;
        let crouch = (60..=95).contains(&phase);
        let jump_pressed = phase == 120;
        let jump_held = (120..=126).contains(&phase);
        let command = MoveCommand::new(
            Vec2::new(0.0, forward),
            ViewAngles::ZERO,
            jump_pressed,
            jump_held,
            if phase == 150 {
                Trigger::Held
            } else {
                Trigger::Released
            },
            phase == 151,
        )?
        .with_movement(sprint, crouch);
        arena.command_player(command)?;
        arena.step()?;
        hashes.push(arena.world.state_hash()?);
    }
    Ok(hashes)
}

#[test]
fn six_hundred_scripted_movement_ticks_match_every_hash() -> TestResult {
    let first = scripted_movement_hashes(191)?;
    let second = scripted_movement_hashes(191)?;
    assert_eq!(first, second, "same commands and seed match at every tick");
    assert_eq!(
        first.last().copied(),
        Some(7_231_586_728_695_893_047),
        "arena geometry, tuning or the hashed state changed from the earlier recording"
    );
    Ok(())
}

#[test]
fn six_hundred_auto_fire_ticks_have_a_recorded_deterministic_hash() -> TestResult {
    let mut arena = Arena::new(191)?;
    for tick in 0..600_u32 {
        let phase = tick % 180;
        let forward = if phase < 110 { 1.0 } else { 0.0 };
        let sprint = phase < 80;
        let crouch = (60..=95).contains(&phase);
        let jump_pressed = phase == 120;
        let jump_held = (120..=126).contains(&phase);
        let command = MoveCommand::new(
            Vec2::new(0.0, forward),
            ViewAngles::ZERO,
            jump_pressed,
            jump_held,
            Trigger::OnTarget,
            phase == 151,
        )?
        .with_movement(sprint, crouch);
        arena.command_player(command)?;
        arena.step()?;
    }
    let hash = arena.world.state_hash()?;
    assert_eq!(
        hash, 12_405_451_002_445_932_230,
        "arena geometry, tuning or the hashed state changed from the earlier recording"
    );
    Ok(())
}

#[test]
fn the_player_spawns_are_the_spawn_points_of_the_scene() -> TestResult {
    // The player starts at its first spawn point; the scripted fights above rely on that.
    let arena = Arena::new(SEED)?;
    let feet = centre_of(&arena.world, PLAYER_SLOT)? - Vec3::Y * imagine::data::CENTRE_ABOVE_FEET_M;
    let spawn = *arena
        .world
        .resource::<ArenaData>()?
        .player_spawns()
        .first()
        .ok_or("player spawn missing")?;
    assert!(feet.distance(spawn) < 1e-3, "{feet:?}");
    Ok(())
}
