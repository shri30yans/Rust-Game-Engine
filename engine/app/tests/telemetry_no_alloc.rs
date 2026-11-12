//! `Telemetry::record` and `Telemetry::snapshot` must not allocate after `new`.
//!
//! One `#[test]` only: the allocator counter is process-wide, so parallel tests would pollute it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{Telemetry, WINDOW_FRAMES};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FRAMES: usize = 4 * WINDOW_FRAMES;

#[test]
fn record_and_snapshot_allocate_nothing_after_new() {
    let mut telemetry = Telemetry::new();
    // Warm up past one full window so every slot and the scratch buffer have been used.
    for frame in 0..WINDOW_FRAMES {
        telemetry
            .record(8.0 + frame as f32 * 0.001, 0.2)
            .expect("valid");
        black_box(telemetry.snapshot().expect("snapshot"));
    }

    let region = Region::new(GLOBAL);
    for frame in 0..FRAMES {
        let frame_ms = black_box(8.0 + (frame % 17) as f32 * 0.5);
        telemetry.record(frame_ms, 0.2).expect("valid");
        black_box(telemetry.snapshot().expect("snapshot"));
    }
    let stats = region.change();

    assert_eq!(stats.allocations, 0, "telemetry allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "telemetry reallocated: {stats:?}");
}
