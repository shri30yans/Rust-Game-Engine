//! Bots fight each other: with the player standing still, bots kill bots (free for all), and a
//! shot fired near an idle bot draws it in.

mod common;

use common::{Arena, TestResult, actor_count};
use engine_world::World;
use imagine::data::TICK_RATE_HZ;
use imagine::rules::{KillFeed, PLAYER_SLOT, Score};
use imagine::sim::is_alive;
use kit_fps::MoveCommand;

const SEED: u64 = 7;

/// How many bots are down right now.
fn bots_down(world: &World) -> Result<usize, Box<dyn std::error::Error>> {
    let mut down = 0;
    for slot in 1..actor_count(world)? {
        down += usize::from(!is_alive(world, slot)?);
    }
    Ok(down)
}

#[test]
fn bots_kill_each_other_without_the_player_firing() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    let mut bot_deaths = 0;
    let mut was_down = 0;
    for _ in 0..(120 * u32::from(TICK_RATE_HZ)) {
        arena.command_player(MoveCommand::IDLE)?;
        arena.step()?;
        let down = bots_down(&arena.world)?;
        bot_deaths += down.saturating_sub(was_down);
        was_down = down;
        if !is_alive(&arena.world, PLAYER_SLOT)? {
            break;
        }
    }
    assert_eq!(
        arena.world.resource::<Score>()?.kills(),
        0,
        "the player never fired"
    );
    assert!(bot_deaths >= 1, "the bots never killed one another");
    Ok(())
}

#[test]
fn bot_kills_are_credited_and_listed_in_the_feed() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    let mut listed = false;
    for _ in 0..(120 * u32::from(TICK_RATE_HZ)) {
        arena.command_player(MoveCommand::IDLE)?;
        arena.step()?;
        listed |= arena
            .world
            .resource::<KillFeed>()?
            .rows()
            .iter()
            .flatten()
            .any(|entry| entry.killer() != PLAYER_SLOT && entry.victim() != entry.killer());
        if !is_alive(&arena.world, PLAYER_SLOT)? {
            break;
        }
    }
    let score = arena.world.resource::<Score>()?;
    let mut bot_kills = 0;
    for slot in 1..actor_count(&arena.world)? {
        bot_kills += score.tally(slot).map_or(0, |t| t.kills());
    }
    assert!(bot_kills >= 1, "no bot was credited with a kill");
    assert!(listed, "a bot kill never reached the feed");
    assert!(score.ticks() >= 1, "the match clock never advanced");
    Ok(())
}
