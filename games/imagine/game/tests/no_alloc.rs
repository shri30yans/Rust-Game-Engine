//! The game's own per-tick systems allocate nothing once warm: rules, feedback capture and the
//! transform sync. This binary holds one test so that the counting allocator sees only its work.
//!
//! A respawn puts a body back through the physics world, which is not claimed allocation-free
//! (D40), so a tick on which an actor respawned is left out of the count; every other tick must
//! allocate nothing.
//!
//! The measured window runs those three systems directly, without physics, bots or the kit's
//! combat: physics is not claimed allocation-free (D40 records that), and the kit measures its own
//! combat. The warm-up runs the full schedule so that every buffer has its steady-state capacity.

mod common;

use common::{Arena, TestResult, dt};
use engine_core::{SimRng, Tick};
use engine_world::SystemContext;
use engine_world::World;
use imagine::feedback::capture_feedback;
use imagine::rules::{Actors, rules_system};
use imagine::sim::sync_transforms;
use stats_alloc::{Region, StatsAlloc};

#[global_allocator]
static GLOBAL: StatsAlloc<std::alloc::System> = StatsAlloc::system();

/// Ticks run through the full schedule before measuring.
const WARM_UP_TICKS: u32 = 120;
/// Ticks measured.
const MEASURED_TICKS: u32 = 600;

#[test]
fn rules_feedback_and_transform_sync_allocate_nothing_after_warm_up() -> TestResult {
    let mut arena = Arena::new(7)?;
    arena.run(WARM_UP_TICKS)?;
    let mut rng = SimRng::new(7, 99);
    let step = dt();

    let region = Region::new(&GLOBAL);
    let mut counted = 0;
    let mut measured_ticks = 0;
    for tick in 0..MEASURED_TICKS {
        let mut ctx = SystemContext {
            tick: Tick::new(tick),
            dt: step,
            rng: &mut rng,
        };
        let respawns = respawn_count(&arena.world)?;
        let before = region.change().allocations;
        rules_system(&mut arena.world, &mut ctx)?;
        capture_feedback(&mut arena.world, &mut ctx)?;
        sync_transforms(&mut arena.world, &mut ctx)?;
        let spent = region.change().allocations - before;
        if respawn_count(&arena.world)? == respawns {
            counted += spent;
            measured_ticks += 1;
        }
    }
    assert!(
        measured_ticks > MEASURED_TICKS / 2,
        "most ticks are measured"
    );
    assert_eq!(
        counted, 0,
        "the game's systems allocated {counted} times over {measured_ticks} ticks"
    );
    Ok(())
}

/// How many respawns every actor has had, summed.
fn respawn_count(world: &World) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(world
        .resource::<Actors>()?
        .slots()
        .iter()
        .map(|actor| actor.respawns())
        .sum())
}
