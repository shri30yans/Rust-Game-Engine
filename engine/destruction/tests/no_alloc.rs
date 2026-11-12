//! Proves that a damage-and-collapse run allocates nothing once its buffers are warm.
//!
//! This file holds a single test on purpose: the counting allocator is process-wide,
//! so a second test running in parallel would pollute the measurement.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::alloc::System;
use std::sync::Arc;

use engine_core::Tick;
use engine_destruction::{EventSink, StructureState};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn collapse_allocates_nothing_after_warm_up() {
    let structure = Arc::new(common::ground_towers(20, 50, 3));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(1 << 16);

    // Warm-up: the first full solve grows the solver's queue to its steady size.
    common::settle(&mut state, &mut sink, 2048);
    sink.clear();

    let region = Region::new(GLOBAL);
    state
        .apply_damage(common::id(0), 100, Tick::ZERO, &mut sink)
        .unwrap();
    common::settle(&mut state, &mut sink, 2048);
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "collapse allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "collapse reallocated: {stats:?}");
    assert!(
        !state.groups().is_empty(),
        "the collapse should have detached a group"
    );

    // Sanity check that the counter really observes allocations on this thread.
    let region = Region::new(GLOBAL);
    let probe: Vec<u8> = Vec::with_capacity(4096);
    let probe_stats = region.change();
    assert!(
        probe_stats.allocations >= 1,
        "counter missed a Vec allocation"
    );
    drop(probe);
}
