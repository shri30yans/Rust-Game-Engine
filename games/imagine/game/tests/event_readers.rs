//! The readers of the tick's events (feedback, hearing, rules and the combat stats) give the same
//! result whichever order they run in: every order is scheduled over the same scripted match and
//! compared at every tick through the state hash, which covers the feedback, the bot brains, the
//! score, the kill feed and the actors' lives.

mod common;

use common::{Arena, TestResult};
use engine_world::SystemFn;
use imagine::feedback::HudEvents;
use imagine::rules::Score;
use imagine::sim::EVENT_READERS;
use kit_fps::{CombatStats, MoveCommand, Trigger};

/// Twenty simulated seconds: long enough for shots, hits, kills and respawns.
const TICKS: u32 = 1200;
const SEED: u64 = 191;

/// What a run produced: the state hash after every tick, and the combat stats (not hashed).
#[derive(Debug, PartialEq)]
struct Outcome {
    hashes: Vec<u64>,
    shots: u64,
    hits: u64,
    kills: u32,
}

fn run(readers: &[(&'static str, SystemFn)]) -> Result<Outcome, Box<dyn std::error::Error>> {
    let mut arena = Arena::with_readers(SEED, readers)?;
    let mut hashes = Vec::with_capacity(usize::try_from(TICKS)?);
    for tick in 0..TICKS {
        let command = MoveCommand::new(
            glam::Vec2::new(0.0, if tick % 240 < 120 { 1.0 } else { 0.0 }),
            app::ViewAngles::new(f32::from(u16::try_from(tick % 360)?).to_radians(), 0.0)?,
            false,
            false,
            Trigger::OnTarget,
            false,
        )?;
        arena.command_player(command)?;
        arena.step()?;
        hashes.push(arena.world.state_hash()?);
    }
    let stats = arena.world.resource::<CombatStats>()?;
    let kills = arena
        .world
        .resource::<Score>()?
        .tallies()
        .iter()
        .map(|tally| tally.kills() + tally.deaths())
        .sum();
    // Touch the feedback so a missing resource fails here rather than passing silently.
    arena.world.resource::<HudEvents>()?;
    Ok(Outcome {
        hashes,
        shots: stats.shots_fired(),
        hits: stats.hits(),
        kills,
    })
}

/// Every ordering of `items`, by Heap's algorithm.
fn permutations<T: Copy>(items: &[T]) -> Vec<Vec<T>> {
    fn heap<T: Copy>(k: usize, items: &mut Vec<T>, out: &mut Vec<Vec<T>>) {
        if k <= 1 {
            out.push(items.clone());
            return;
        }
        for i in 0..k {
            heap(k - 1, items, out);
            let swap_with = if k.is_multiple_of(2) { i } else { 0 };
            items.swap(swap_with, k - 1);
        }
    }
    let mut work = items.to_vec();
    let mut out = Vec::new();
    heap(work.len(), &mut work, &mut out);
    out
}

#[test]
fn every_order_of_the_event_readers_gives_the_same_match() -> TestResult {
    let orders = permutations(&EVENT_READERS);
    assert_eq!(orders.len(), 24);
    let reference = run(&EVENT_READERS)?;
    assert!(reference.shots > 0, "nobody fired");
    assert!(reference.hits > 0, "nobody hit");
    assert!(
        reference.kills > 0,
        "nobody died, so the rules read no kill"
    );
    for order in &orders {
        let outcome = run(order)?;
        let names: Vec<&str> = order.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            outcome, reference,
            "reader order {names:?} changed the match"
        );
    }
    Ok(())
}
