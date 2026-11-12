//! Criterion benchmarks: transform propagation and state hashing over 50k entities.
//!
//! The hierarchy is 500 chains of 100 entities each, so propagation walks depth
//! 100 and visits every entity once per call.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use engine_core::SimRng;
use engine_world::{NetId, SystemContext, Transform, World, propagate_transforms};
use glam::{Quat, Vec3};

const ENTITY_COUNT: usize = 50_000;
const CHAIN_LENGTH: usize = 100;

fn build_world() -> (World, Vec<NetId>) {
    let mut world = World::new(0xB0B).unwrap();
    let mut ids = Vec::with_capacity(ENTITY_COUNT);
    for i in 0..ENTITY_COUNT {
        let id = world.spawn().unwrap();
        let phase = i as f32 * 0.001;
        let transform = Transform::new(
            Vec3::new(phase.sin() * 10.0, phase.cos() * 10.0, phase),
            Quat::from_rotation_y(phase),
            Vec3::ONE,
        )
        .unwrap();
        *world.get_mut::<Transform>(id).unwrap() = transform;
        let parent = ids.last().copied().filter(|_| i % CHAIN_LENGTH != 0);
        if let Some(previous) = parent {
            world.set_parent(id, previous).unwrap();
        }
        ids.push(id);
    }
    (world, ids)
}

fn bench_propagate(c: &mut Criterion) {
    let (mut world, _) = build_world();
    let mut rng = SimRng::new(2, 0);
    let mut ctx = SystemContext {
        tick: engine_core::Tick::ZERO,
        dt: 1.0 / 60.0,
        rng: &mut rng,
    };
    // Warm-up grows the traversal buffer so the measured calls reuse it.
    propagate_transforms(&mut world, &mut ctx).unwrap();
    c.bench_function("propagate_transforms_50k", |b| {
        b.iter(|| {
            propagate_transforms(black_box(&mut world), &mut ctx).unwrap();
        });
    });
}

fn bench_state_hash(c: &mut Criterion) {
    let (world, _) = build_world();
    c.bench_function("state_hash_50k", |b| {
        b.iter(|| black_box(world.state_hash().unwrap()));
    });
}

criterion_group!(benches, bench_propagate, bench_state_hash);
criterion_main!(benches);
