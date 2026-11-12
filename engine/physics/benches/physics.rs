//! Criterion benchmarks for a 500-body step and 64 shared character moves.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use engine_physics::{
    BodyBuilder, CharacterConfig, CharacterController, CharacterInput, CharacterState,
    ColliderBuilder, ColliderShape, ConvexShape, PhysicsConfig, PhysicsWorld, QueryFilter,
    TriMeshShape,
};
use glam::Vec3;

fn dynamic_world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(50.0, 0.5, 50.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    for i in 0..500 {
        let x = (i % 10) as f32 * 1.1 - 5.0;
        let y = (i / 100) as f32 * 1.1 + 1.0;
        let z = ((i / 10) % 10) as f32 * 1.1 - 5.0;
        let body = world.insert_body(
            BodyBuilder::dynamic()
                .translation(Vec3::new(x, y, z))
                .unwrap(),
        );
        world
            .insert_collider(
                Some(body),
                ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(0.5)).unwrap()),
            )
            .unwrap();
    }
    world.step();
    world
}

fn character_world() -> (
    PhysicsWorld,
    CharacterController,
    Vec<CharacterState>,
    Vec<engine_physics::BodyId>,
) {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(100.0, 0.5, 100.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let config = CharacterConfig::capsule(0.5, 0.4).unwrap();
    let controller = CharacterController::new(config);
    let mut states = Vec::with_capacity(64);
    let mut bodies = Vec::with_capacity(64);
    for i in 0..64 {
        let x = (i % 8) as f32 * 2.0;
        let z = (i / 8) as f32 * 2.0;
        let position = Vec3::new(x, 1.0, z);
        let body = world.insert_body(BodyBuilder::kinematic().translation(position).unwrap());
        world
            .insert_collider(Some(body), ColliderBuilder::new(config.shape()))
            .unwrap();
        states.push(CharacterState::with_motion(position, 0.0, true).unwrap());
        bodies.push(body);
    }
    (world, controller, states, bodies)
}

fn bench_dynamic_step(c: &mut Criterion) {
    c.bench_function("physics_step_500_active_dynamic_bodies", |b| {
        b.iter_batched(
            dynamic_world,
            |mut world| world.step(),
            criterion::BatchSize::LargeInput,
        );
    });
}

fn bench_character_moves(c: &mut Criterion) {
    let (world, controller, states, bodies) = character_world();
    let input = CharacterInput::new(Vec3::new(4.0, 0.0, 1.0), false).unwrap();
    c.bench_function("physics_64_character_moves", |b| {
        b.iter(|| {
            for (state, body) in states.iter().zip(&bodies) {
                black_box(controller.move_character(*state, input, &world, *body));
            }
        });
    });
}

fn raycast_world_50k_triangles() -> PhysicsWorld {
    let columns = 250_u32;
    let rows = 100_u32;
    let mut vertices = Vec::with_capacity(25_551);
    for z in 0..=rows {
        for x in 0..=columns {
            vertices.push(Vec3::new(
                f32::from(u16::try_from(x).unwrap()) * 0.1,
                0.0,
                f32::from(u16::try_from(z).unwrap()) * 0.1,
            ));
        }
    }
    let mut indices = Vec::with_capacity(50_000);
    for z in 0..rows {
        for x in 0..columns {
            let a = z * (columns + 1) + x;
            let b = a + 1;
            let c = a + columns + 1;
            let d = c + 1;
            indices.push([a, c, b]);
            indices.push([b, c, d]);
        }
    }
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let mesh = TriMeshShape::new(vertices, indices).unwrap();
    world
        .insert_collider(None, ColliderBuilder::new(ColliderShape::TriMesh(mesh)))
        .unwrap();
    world
}

fn bench_raycast_50k_triangles(c: &mut Criterion) {
    let world = raycast_world_50k_triangles();
    let ray =
        engine_physics::RayCast::new(Vec3::new(12.4, 10.0, 7.4), -Vec3::Y, 20.0, true).unwrap();
    c.bench_function("physics_ray_cast_50000_triangle_mesh", |b| {
        b.iter(|| black_box(world.ray_cast(ray, QueryFilter::default())));
    });
}

criterion_group!(
    benches,
    bench_dynamic_step,
    bench_character_moves,
    bench_raycast_50k_triangles
);
criterion_main!(benches);
