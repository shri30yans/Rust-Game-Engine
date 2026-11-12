//! The governor's `observe` must not allocate after `Governor::new`, since it runs once per frame.
//!
//! One `#[test]` only: the allocator counter is process-wide, so parallel tests would pollute it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use render::{Governor, GovernorConfig};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FRAMES: u32 = 2000;

#[test]
fn observe_allocates_nothing_after_construction() {
    let config = GovernorConfig::new(16.6, 0.5, 1.0).expect("valid config");
    let mut governor = Governor::new(config, 1.0).expect("valid start");
    // Warm up through a full window and a change so every path has run once.
    for frame in 0..60u32 {
        black_box(
            governor
                .observe(black_box(if frame < 30 { 30.0 } else { 5.0 }))
                .expect("finite"),
        );
    }

    let region = Region::new(GLOBAL);
    for frame in 0..FRAMES {
        // Alternate slow and fast stretches so both raising and lowering run in the window.
        let ms = if (frame / 90) % 2 == 0 { 30.0 } else { 5.0 };
        black_box(governor.observe(black_box(ms)).expect("finite"));
    }
    let stats = region.change();

    assert_eq!(stats.allocations, 0, "observe allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "observe reallocated: {stats:?}");
}
