//! Repeat-run and golden-hash determinism tests over 600 fixed ticks.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_core::SimRng;
use engine_physics::{BodyBuilder, ColliderBuilder, ConvexShape, PhysicsConfig, PhysicsWorld};
use glam::Vec3;

const SEED: u64 = 0x51A7_E600_D15C_A11E;
// Independently computed FNV-1a digest of the empty world: one closing `0` tag byte and no bodies.
const GOLDEN_EMPTY_WORLD_HASH: u64 = 0xaf63_bd4c_8601_b7df;

fn run(seed: u64) -> Vec<u64> {
    let mut rng = SimRng::new(seed, 3);
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(30.0, 0.5, 30.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap()
                .friction(0.8)
                .unwrap(),
        )
        .unwrap();

    for i in 0..32 {
        let x = rng.range_f32(-6.0, 6.0).unwrap();
        let z = rng.range_f32(-6.0, 6.0).unwrap();
        let y = 1.0 + i as f32 * 0.8;
        let body = world.insert_body(
            BodyBuilder::dynamic()
                .translation(Vec3::new(x, y, z))
                .unwrap()
                .linear_velocity(Vec3::new(
                    rng.range_f32(-1.0, 1.0).unwrap(),
                    0.0,
                    rng.range_f32(-1.0, 1.0).unwrap(),
                ))
                .unwrap()
                .continuous_collision_detection(true),
        );
        let shape = if i % 2 == 0 {
            ConvexShape::ball(0.35).unwrap()
        } else {
            ConvexShape::cuboid(Vec3::splat(0.35)).unwrap()
        };
        world
            .insert_collider(Some(body), ColliderBuilder::new(shape))
            .unwrap();
    }

    let mut hashes = Vec::with_capacity(600);
    for _ in 0..600 {
        world.step();
        hashes.push(world.state_hash());
    }
    hashes
}

#[test]
fn same_seed_and_inputs_match_every_tick_for_600_ticks() {
    assert_eq!(run(SEED), run(SEED));
}

#[test]
fn empty_world_hash_matches_golden_after_600_ticks() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    for _ in 0..600 {
        world.step();
    }
    assert_eq!(world.state_hash(), GOLDEN_EMPTY_WORLD_HASH);
}
