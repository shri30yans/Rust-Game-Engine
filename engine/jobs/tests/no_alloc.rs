//! Hot-path chunked helpers must not allocate on the inline pool after warm-up.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use jobs::{ChunkSize, JobPool};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ITEMS: usize = 4096;
const CHUNK_ITEMS: usize = 256;

// The inline pool allocates nothing after warm-up, exactly. The rayon pool's own code allocates
// nothing either, but the global counting allocator also sees rayon/crossbeam internals on worker
// threads: the job injector allocates a new block roughly every 63 externally injected jobs, so a
// single call can see a few allocations depending on thread timing. The rayon bound is therefore
// amortized over many warmed calls (measured: well under one allocation per 32 calls).
const RAYON_WARM_UP_CALLS: usize = 64;
const RAYON_MEASURED_CALLS: usize = 1000;
const RAYON_MAX_ALLOCATIONS_PER_1000_CALLS: usize = 31;

fn assert_rayon_stats(name: &str, stats: stats_alloc::Stats) {
    println!("{name}: {stats:?} over {RAYON_MEASURED_CALLS} calls");
    assert!(
        stats.allocations <= RAYON_MAX_ALLOCATIONS_PER_1000_CALLS,
        "{name} allocations: {stats:?} (cap {RAYON_MAX_ALLOCATIONS_PER_1000_CALLS} per {RAYON_MEASURED_CALLS} calls)"
    );
    assert_eq!(stats.reallocations, 0, "{name}: {stats:?}");
}

#[test]
fn warmed_chunk_helpers_allocate_bounded() {
    let chunk = ChunkSize::new(CHUNK_ITEMS).unwrap();
    let mut items = vec![1.0_f64; ITEMS];
    let slots = items.len().div_ceil(CHUNK_ITEMS);
    let mut out = vec![0.0_f64; slots];

    let inline = JobPool::new(0).unwrap();
    inline.for_each_chunk_mut(&mut items, chunk, |_, chunk_items| {
        for value in chunk_items {
            *value += 1.0;
        }
    });
    inline
        .map_chunks(&items, chunk, &mut out, |_, chunk_items| {
            let mut acc = 0.0;
            for &v in chunk_items {
                acc += v;
            }
            acc
        })
        .unwrap();
    black_box(out.as_slice());

    let inline_region = Region::new(GLOBAL);
    inline.for_each_chunk_mut(&mut items, chunk, |_, chunk_items| {
        for value in chunk_items {
            *value += 1.0;
        }
    });
    let inline_foreach_stats = inline_region.change();
    assert_eq!(
        inline_foreach_stats.allocations, 0,
        "inline for_each_chunk_mut: {inline_foreach_stats:?}"
    );
    assert_eq!(
        inline_foreach_stats.reallocations, 0,
        "inline for_each_chunk_mut: {inline_foreach_stats:?}"
    );

    let inline_map_region = Region::new(GLOBAL);
    inline
        .map_chunks(&items, chunk, &mut out, |_, chunk_items| {
            let mut acc = 0.0;
            for &v in chunk_items {
                acc += v;
            }
            acc
        })
        .unwrap();
    black_box(out.as_slice());
    let inline_map_stats = inline_map_region.change();
    assert_eq!(
        inline_map_stats.allocations, 0,
        "inline map_chunks: {inline_map_stats:?}"
    );
    assert_eq!(
        inline_map_stats.reallocations, 0,
        "inline map_chunks: {inline_map_stats:?}"
    );

    let rayon = JobPool::new(2).unwrap();
    let rayon_foreach = |items: &mut [f64]| {
        rayon.for_each_chunk_mut(items, chunk, |_, chunk_items| {
            for value in chunk_items {
                *value += 1.0;
            }
        });
    };
    let rayon_map = |items: &[f64], out: &mut [f64]| {
        rayon
            .map_chunks(items, chunk, out, |_, chunk_items| {
                let mut acc = 0.0;
                for &v in chunk_items {
                    acc += v;
                }
                acc
            })
            .unwrap();
        black_box(&*out);
    };
    for _ in 0..RAYON_WARM_UP_CALLS {
        rayon_foreach(&mut items);
        rayon_map(&items, &mut out);
    }

    let rayon_foreach_region = Region::new(GLOBAL);
    for _ in 0..RAYON_MEASURED_CALLS {
        rayon_foreach(&mut items);
    }
    let rayon_foreach_stats = rayon_foreach_region.change();

    let rayon_map_region = Region::new(GLOBAL);
    for _ in 0..RAYON_MEASURED_CALLS {
        rayon_map(&items, &mut out);
    }
    let rayon_map_stats = rayon_map_region.change();

    assert_rayon_stats("rayon for_each_chunk_mut", rayon_foreach_stats);
    assert_rayon_stats("rayon map_chunks", rayon_map_stats);

    let probe_region = Region::new(GLOBAL);
    let _probe: Vec<u8> = Vec::with_capacity(64);
    assert!(
        probe_region.change().allocations >= 1,
        "allocator counter must observe allocations"
    );
}
