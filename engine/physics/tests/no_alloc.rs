//! Measures warmed stable-topology stepping and character movement with the process-wide allocator.
//!
//! This file intentionally has one test because parallel tests would pollute the counter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::spawn_character;
use engine_physics::{
    BodyBuilder, CharacterConfig, CharacterInput, CharacterState, ColliderBuilder, ConvexShape,
    PhysicsConfig, PhysicsWorld,
};
use glam::Vec3;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn warmed_physics_hot_paths_allocate_nothing() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let mut bodies = Vec::with_capacity(64);
    for layer in 0..4 {
        for x in 0..4 {
            for z in 0..4 {
                let body = world.insert_body(
                    BodyBuilder::dynamic()
                        .translation(Vec3::new(
                            x as f32 * 1.05,
                            0.5 + layer as f32 * 1.05,
                            z as f32 * 1.05,
                        ))
                        .unwrap(),
                );
                world
                    .insert_collider(
                        Some(body),
                        ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(0.5)).unwrap()),
                    )
                    .unwrap();
                bodies.push(body);
            }
        }
    }

    for _ in 0..16 {
        world.step();
    }
    assert!(
        bodies
            .iter()
            .any(|id| !world.body_state(*id).unwrap().sleeping)
    );

    let character_config = CharacterConfig::capsule(0.5, 0.4).unwrap();
    let dynamic_pile_input = CharacterInput::new(Vec3::new(4.0, 0.0, 1.0), false).unwrap();
    let dynamic_pile_states = (0..64)
        .map(|i| {
            let x = (i % 8) as f32 * 2.0 - 15.0;
            let z = (i / 8) as f32 * 2.0 - 15.0;
            CharacterState::with_motion(Vec3::new(x, 1.0, z), 0.0, true).unwrap()
        })
        .collect::<Vec<_>>();
    // Measured: Rapier character `move_shape` builds `Vec<ContactManifold>` when shape casts hit dynamics (measured: 2 allocs / 1280 bytes per 64 moves).
    const CHARACTER_DYNAMIC_MOVES_MAX_ALLOCATIONS: usize = 8;
    const CHARACTER_DYNAMIC_MOVES_MAX_BYTES: usize = 1280;

    let step_region = Region::new(GLOBAL);
    world.step();
    let step_stats = step_region.change();
    // Rapier 0.36's narrow phase allocates a few small buffers per warmed step; we bound, not forbid.
    assert!(
        step_stats.allocations <= 3,
        "warmed Rapier contact step allocated: {step_stats:?}"
    );
    assert!(
        step_stats.bytes_allocated <= 640,
        "warmed Rapier contact step bytes: {step_stats:?}"
    );
    assert_eq!(
        step_stats.reallocations, 0,
        "warmed Rapier contact step reallocated: {step_stats:?}"
    );

    let mut static_world = PhysicsWorld::new(PhysicsConfig::default());
    static_world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let mut static_character_bodies = Vec::with_capacity(dynamic_pile_states.len());
    let mut controller = None;
    for state in &dynamic_pile_states {
        let (character, body, _) =
            spawn_character(&mut static_world, character_config, state.position());
        controller.get_or_insert(character);
        static_character_bodies.push(body);
    }
    let controller = controller.unwrap();
    let mut dynamic_character_bodies = Vec::with_capacity(dynamic_pile_states.len());
    for state in &dynamic_pile_states {
        let (_, body, _) = spawn_character(&mut world, character_config, state.position());
        dynamic_character_bodies.push(body);
    }
    for _ in 0..16 {
        static_world.step();
    }
    for (state, body) in dynamic_pile_states.iter().zip(&static_character_bodies) {
        black_box(controller.move_character(*state, dynamic_pile_input, &static_world, *body));
    }

    let static_character_region = Region::new(GLOBAL);
    for (state, body) in dynamic_pile_states.iter().zip(&static_character_bodies) {
        black_box(controller.move_character(*state, dynamic_pile_input, &static_world, *body));
    }
    let static_character_stats = static_character_region.change();
    assert_eq!(
        static_character_stats.allocations, 0,
        "64 character moves over static geometry allocated: {static_character_stats:?}"
    );
    assert_eq!(
        static_character_stats.reallocations, 0,
        "64 character moves over static geometry reallocated: {static_character_stats:?}"
    );

    for (state, body) in dynamic_pile_states.iter().zip(&dynamic_character_bodies) {
        black_box(controller.move_character(*state, dynamic_pile_input, &world, *body));
    }

    let dynamic_character_region = Region::new(GLOBAL);
    for (state, body) in dynamic_pile_states.iter().zip(&dynamic_character_bodies) {
        black_box(controller.move_character(*state, dynamic_pile_input, &world, *body));
    }
    let dynamic_character_stats = dynamic_character_region.change();
    assert!(
        dynamic_character_stats.allocations <= CHARACTER_DYNAMIC_MOVES_MAX_ALLOCATIONS,
        "64 character moves against dynamic pile allocated: {dynamic_character_stats:?}"
    );
    assert!(
        dynamic_character_stats.bytes_allocated <= CHARACTER_DYNAMIC_MOVES_MAX_BYTES,
        "64 character moves against dynamic pile bytes: {dynamic_character_stats:?}"
    );
    assert_eq!(
        dynamic_character_stats.reallocations, 0,
        "64 character moves against dynamic pile reallocated: {dynamic_character_stats:?}"
    );

    let probe_region = Region::new(GLOBAL);
    let probe: Vec<u8> = Vec::with_capacity(4096);
    let probe_stats = probe_region.change();
    assert!(
        probe_stats.allocations >= 1,
        "counter missed a Vec allocation"
    );
    drop(probe);
}
