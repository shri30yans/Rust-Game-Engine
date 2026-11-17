//! Proves that the per-tick event logs allocate nothing at steady state: a log sized at
//! registration is written, read and cleared by the schedule every tick without touching the heap.
//!
//! This file holds a single test on purpose: the counting allocator is process-wide,
//! so a second test running in parallel would pollute the measurement.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_core::{SimRng, Tick};
use engine_world::{Schedule, SystemContext, World, WorldError};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Actors in the measured world, and events each writes per tick: the data the capacity comes from.
const ACTORS: usize = 24;
const EVENTS_PER_ACTOR: usize = 3;

#[derive(Debug, Clone, Copy)]
struct Hit {
    amount: u32,
}

fn write_hits(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    for actor in 0..ACTORS * EVENTS_PER_ACTOR {
        let amount = u32::try_from(actor).unwrap_or(0) + ctx.tick.raw();
        world.send(Hit { amount })?;
    }
    Ok(())
}

fn read_hits(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let total: u32 = world.events::<Hit>()?.iter().map(|hit| hit.amount).sum();
    assert!(total > 0);
    Ok(())
}

#[test]
fn event_logs_allocate_nothing_at_steady_state() {
    let mut world = World::new(3).unwrap();
    world.add_events::<Hit>(ACTORS * EVENTS_PER_ACTOR).unwrap();
    let mut schedule = Schedule::new(&SimRng::new(3, 0), 1.0 / 60.0).unwrap();
    schedule.add_system("write", write_hits).unwrap();
    schedule.add_system("read.a", read_hits).unwrap();
    schedule.add_system("read.b", read_hits).unwrap();

    let region = Region::new(GLOBAL);
    for tick in 0..600 {
        schedule.run_tick(&mut world, Tick::new(tick)).unwrap();
    }
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "event logs allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "event logs reallocated: {stats:?}");
    assert_eq!(
        world.events::<Hit>().unwrap().len(),
        ACTORS * EVENTS_PER_ACTOR
    );

    // Sanity check that the counter really observes allocations on this thread.
    let region = Region::new(GLOBAL);
    let probe: Vec<u8> = Vec::with_capacity(4096);
    assert!(
        region.change().allocations >= 1,
        "counter missed a Vec allocation"
    );
    drop(probe);
}
