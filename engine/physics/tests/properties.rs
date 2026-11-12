//! Property tests for finite rigid-body state and collision-safe character movement.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::spawn_character;
use engine_physics::{
    BodyBuilder, CharacterConfig, CharacterInput, CharacterState, ColliderBuilder, ColliderShape,
    ConvexShape, PhysicsConfig, PhysicsWorld, QueryFilter, TriMeshShape,
};
use glam::{Quat, Vec3};
use proptest::prelude::*;

fn triangle_mesh_room(width: f32, depth: f32) -> TriMeshShape {
    let half_width = width * 0.5;
    let half_depth = depth * 0.5;
    let height = 3.0;
    let quads = [
        [
            Vec3::new(-half_width, 0.0, -half_depth),
            Vec3::new(half_width, 0.0, -half_depth),
            Vec3::new(-half_width, 0.0, half_depth),
            Vec3::new(half_width, 0.0, half_depth),
        ],
        [
            Vec3::new(-half_width, 0.0, -half_depth),
            Vec3::new(-half_width, height, -half_depth),
            Vec3::new(-half_width, 0.0, half_depth),
            Vec3::new(-half_width, height, half_depth),
        ],
        [
            Vec3::new(half_width, 0.0, half_depth),
            Vec3::new(half_width, height, half_depth),
            Vec3::new(half_width, 0.0, -half_depth),
            Vec3::new(half_width, height, -half_depth),
        ],
        [
            Vec3::new(half_width, 0.0, -half_depth),
            Vec3::new(half_width, height, -half_depth),
            Vec3::new(-half_width, 0.0, -half_depth),
            Vec3::new(-half_width, height, -half_depth),
        ],
        [
            Vec3::new(-half_width, 0.0, half_depth),
            Vec3::new(-half_width, height, half_depth),
            Vec3::new(half_width, 0.0, half_depth),
            Vec3::new(half_width, height, half_depth),
        ],
    ];
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut triangles = Vec::with_capacity(quads.len() * 2);
    for [a, b, c, d] in quads {
        let offset = u32::try_from(vertices.len()).unwrap();
        vertices.extend([a, b, c, d]);
        triangles.push([offset, offset + 2, offset + 1]);
        triangles.push([offset + 1, offset + 2, offset + 3]);
    }
    TriMeshShape::new(vertices, triangles).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn bodies_remain_finite_for_finite_initial_conditions(
        px in -10.0f32..10.0,
        py in 1.0f32..20.0,
        pz in -10.0f32..10.0,
        vx in -20.0f32..20.0,
        vy in -20.0f32..20.0,
        vz in -20.0f32..20.0,
    ) {
        let mut world = PhysicsWorld::new(PhysicsConfig::default());
        world.insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(30.0, 0.5, 30.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0)).unwrap(),
        ).unwrap();
        let body = world.insert_body(
            BodyBuilder::dynamic()
                .translation(Vec3::new(px, py, pz)).unwrap()
                .linear_velocity(Vec3::new(vx, vy, vz)).unwrap()
                .continuous_collision_detection(true),
        );
        world.insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::ball(0.5).unwrap())
                .restitution(0.5).unwrap(),
        ).unwrap();

        for _ in 0..240 {
            world.step();
            let state = world.body_state(body).unwrap();
            prop_assert!(state.translation.is_finite());
            prop_assert!(state.rotation.is_finite());
            prop_assert!(state.linear_velocity.is_finite());
            prop_assert!(state.angular_velocity.is_finite());
        }
    }

    #[test]
    fn character_never_ends_inside_random_static_wall_or_ramp(
        obstacle_x in 1.5f32..6.0,
        angle in 0.0f32..0.65,
        speed in 0.5f32..12.0,
        use_ramp in any::<bool>(),
    ) {
        let mut world = PhysicsWorld::new(PhysicsConfig::default());
        world.insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0)).unwrap(),
        ).unwrap();

        let obstacle_rotation = if use_ramp {
            Quat::from_rotation_z(angle)
        } else {
            Quat::IDENTITY
        };
        world.insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(0.4, 2.0, 3.0)).unwrap())
                .translation(Vec3::new(obstacle_x, 1.5, 0.0)).unwrap()
                .rotation(obstacle_rotation).unwrap(),
        ).unwrap();

        let config = CharacterConfig::capsule(0.5, 0.4).unwrap()
            .step_up(0.25, 0.2).unwrap()
            .slope_limit(0.7).unwrap();
        // Rapier keeps the controller shape `offset` (default skin width) away from surfaces, so the
        // outer capsule can geometrically overlap by at most that amount after a move. Inset by the same
        // default skin width so deeper embedding still fails while floor contact within the gap passes.
        const SKIN_WIDTH: f32 = 0.01;
        let clearance_shape =
            ConvexShape::capsule(0.5, 0.4 - SKIN_WIDTH).expect("positive inset radius");
        let (controller, body, initial_state) =
            spawn_character(&mut world, config, Vec3::new(0.0, 1.0, 0.0));
        let mut state = CharacterState::with_motion(
            initial_state.position(),
            0.0,
            true,
        ).unwrap();
        let input = CharacterInput::new(Vec3::new(speed, 0.0, 0.0), false).unwrap();

        for _ in 0..120 {
            let movement = controller.move_character(state, input, &world, body);
            state = movement.state;
            prop_assert!(state.position().is_finite());
            prop_assert!(
                !world
                    .intersects_shape(
                        clearance_shape.clone(),
                        state.position(),
                        Quat::IDENTITY,
                        QueryFilter::default().excluding_body(body),
                    )
                    .unwrap()
            );
        }
    }

    #[test]
    fn character_stays_within_the_skin_in_random_triangle_mesh_rooms(
        room_width in 4.0f32..8.0,
        room_depth in 4.0f32..8.0,
        start_x in -1.0f32..1.0,
        start_z in -1.0f32..1.0,
        speed_x in -12.0f32..12.0,
        speed_z in -12.0f32..12.0,
    ) {
        let mut world = PhysicsWorld::new(PhysicsConfig::default());
        world.insert_collider(
            None,
            ColliderBuilder::new(ColliderShape::TriMesh(
                triangle_mesh_room(room_width, room_depth),
            )),
        ).unwrap();
        let config = CharacterConfig::capsule(0.5, 0.4).unwrap()
            .step_up(0.25, 0.2).unwrap();
        let (controller, body, initial_state) = spawn_character(
            &mut world,
            config,
            Vec3::new(start_x, 0.91, start_z),
        );
        let mut state = CharacterState::with_motion(initial_state.position(), 0.0, true).unwrap();
        let input = CharacterInput::new(Vec3::new(speed_x, 0.0, speed_z), false).unwrap();
        const SKIN_WIDTH: f32 = 0.01;
        let clearance_shape = ConvexShape::capsule(0.5, 0.4 - SKIN_WIDTH).unwrap();

        for _ in 0..120 {
            state = controller.move_character(state, input, &world, body).state;
            prop_assert!(state.position().is_finite());
            prop_assert!(
                !world.intersects_shape(
                    clearance_shape.clone(),
                    state.position(),
                    Quat::IDENTITY,
                    QueryFilter::default().excluding_body(body),
                ).unwrap(),
                "character penetrated more than the {SKIN_WIDTH} m skin in a {room_width}x{room_depth} m triangle-mesh room"
            );
        }
    }
}
