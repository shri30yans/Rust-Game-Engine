//! Proves that transform propagation allocates nothing once its traversal buffer is warm.
//!
//! This file holds a single test on purpose: the counting allocator is process-wide,
//! so a second test running in parallel would pollute the measurement.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_core::{SimRng, Tick};
use engine_world::{NetId, SystemContext, Transform, World, propagate_transforms};
use glam::{Quat, Vec3};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn build_chains(world: &mut World, chains: usize, length: usize) {
    for chain in 0..chains {
        let mut previous: Option<NetId> = None;
        for depth in 0..length {
            let id = world.spawn().unwrap();
            let phase = (chain * length + depth) as f32 * 0.01;
            let transform = Transform::new(
                Vec3::new(phase.sin(), phase.cos(), phase),
                Quat::from_rotation_y(phase),
                Vec3::ONE,
            )
            .unwrap();
            *world.get_mut::<Transform>(id).unwrap() = transform;
            if let Some(parent) = previous {
                world.set_parent(id, parent).unwrap();
            }
            previous = Some(id);
        }
    }
}

#[test]
fn propagation_allocates_nothing_after_warm_up() {
    let mut world = World::new(11).unwrap();
    build_chains(&mut world, 20, 50);
    let mut rng = SimRng::new(11, 0);
    let mut ctx = SystemContext {
        tick: Tick::ZERO,
        dt: 1.0 / 60.0,
        rng: &mut rng,
    };

    // Warm-up: the first call may grow the traversal buffer.
    propagate_transforms(&mut world, &mut ctx).unwrap();

    let region = Region::new(GLOBAL);
    propagate_transforms(&mut world, &mut ctx).unwrap();
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "propagation allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "propagation reallocated: {stats:?}");

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
