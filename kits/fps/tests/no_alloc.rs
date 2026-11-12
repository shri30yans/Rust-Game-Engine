//! Proves the kit's combat path allocates nothing once warm: a counting allocator wraps the system
//! allocator, and the bot, weapon and damage systems run together over a warmed-up duel inside a
//! measured region. In the measured ticks the bot senses, aims, fires and takes hits.
//!
//! Scope: the locomotion system and the physics step are not measured here. Their allocations come
//! from the rapier-backed character controller and query pipeline in `engine-physics`, which the
//! kit calls but does not own (see `engine/physics/tests/no_alloc.rs`).

#[allow(
    dead_code,
    reason = "each test binary uses a subset of the shared scene builders"
)]
mod common;

use std::alloc::System;
use std::error::Error;

use engine_core::{SimRng, Tick};
use engine_world::Schedule;
use stats_alloc::{Region, StatsAlloc};

#[global_allocator]
static GLOBAL: StatsAlloc<System> = StatsAlloc::system();

/// Ticks run before measuring, so one-off growth of buffers (the roster, the damage log) is done.
const WARM_UP_TICKS: u32 = 120;
/// Ticks measured: several seconds of a live fight with shots, reloads and aiming.
const MEASURED_TICKS: u32 = 600;

#[test]
fn the_combat_path_allocates_nothing_once_warm() -> Result<(), Box<dyn Error>> {
    let mut duel = common::duel(5, false)?;
    let mut schedule = Schedule::new(&SimRng::new(5, 0), common::DT)?;
    schedule.add_system("bot", kit_fps::bot_system)?;
    schedule.add_system("weapon", kit_fps::weapon_system)?;
    schedule.add_system("damage", kit_fps::damage_system)?;
    for tick in 0..WARM_UP_TICKS {
        schedule.run_tick(&mut duel.world, Tick::new(tick))?;
    }
    let region = Region::new(&GLOBAL);
    for tick in WARM_UP_TICKS..WARM_UP_TICKS + MEASURED_TICKS {
        schedule.run_tick(&mut duel.world, Tick::new(tick))?;
    }
    let allocations = region.change().allocations;
    // The measurement only means something if shots were fired inside the measured window.
    let ammo = duel.world.get::<kit_fps::Weapon>(duel.bot)?.state().ammo();
    assert!(ammo < 30, "the bot did not fire in the measured window");
    assert_eq!(
        allocations, 0,
        "the combat path allocated {allocations} times"
    );
    Ok(())
}
