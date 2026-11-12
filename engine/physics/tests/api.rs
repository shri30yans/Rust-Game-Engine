//! Public API unit and integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::spawn_character;
use engine_physics::{
    BodyBuilder, CharacterConfig, CharacterController, CharacterInput, CharacterState,
    ColliderBuilder, ColliderShape, CollisionLayers, CompoundShape, ConvexShape, LayerMask,
    PhysicsConfig, PhysicsError, PhysicsWorld, QueryFilter, RayCast, ShapeCast, SurfaceId,
    TriMeshShape,
};
use engine_world::World;
use glam::{Quat, Vec3};

#[test]
fn configuration_and_builders_reject_invalid_values() {
    assert!(PhysicsConfig::new(0.0, Vec3::ZERO).is_err());
    assert!(PhysicsConfig::new(1.0 / 60.0, Vec3::splat(f32::NAN)).is_err());
    assert!(ConvexShape::ball(0.0).is_err());
    assert!(ConvexShape::cuboid(Vec3::new(1.0, -1.0, 1.0)).is_err());
    assert!(ConvexShape::capsule(-0.1, 0.5).is_err());
    assert!(
        BodyBuilder::dynamic()
            .translation(Vec3::splat(f32::INFINITY))
            .is_err()
    );
    assert!(
        BodyBuilder::dynamic()
            .rotation(Quat::from_xyzw(0.0, 0.0, 0.0, 0.0))
            .is_err()
    );
    assert!(CharacterConfig::capsule(0.5, 0.0).is_err());
    assert!(CharacterInput::new(Vec3::splat(f32::NAN), false).is_err());
}

#[test]
fn convex_hull_mesh_and_compound_constructors_reject_bad_geometry() {
    assert!(ConvexShape::hull(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).is_err());
    assert!(TriMeshShape::new(vec![Vec3::ZERO, Vec3::X, Vec3::Z], vec![[0, 1, 9]]).is_err());
    assert!(
        TriMeshShape::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::splat(f32::NAN)],
            vec![[0, 1, 2]]
        )
        .is_err()
    );
    assert!(
        TriMeshShape::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::new(2.0, 0.0, 0.0)],
            vec![[0, 1, 2]],
        )
        .is_err()
    );
    assert!(CompoundShape::new(Vec::new()).is_err());
    let part = ConvexShape::ball(0.25).unwrap();
    let parts = (0..65)
        .map(|_| (Vec3::ZERO, Quat::IDENTITY, part.clone()))
        .collect();
    assert!(CompoundShape::new(parts).is_err());
}

#[test]
fn dynamic_bodies_reject_triangle_mesh_colliders() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let body = world.insert_body(BodyBuilder::dynamic());
    let mesh = TriMeshShape::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]]).unwrap();
    assert_eq!(
        world.insert_collider(
            Some(body),
            ColliderBuilder::new(ColliderShape::TriMesh(mesh))
        ),
        Err(PhysicsError::DynamicTriangleMesh)
    );
}

#[test]
fn shared_shapes_layers_sensors_and_surfaces_are_observed_by_queries() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let shape = ConvexShape::cuboid(Vec3::splat(0.5)).unwrap();
    let layers = CollisionLayers {
        member: LayerMask::from_bits(1 << 2),
        collides_with: LayerMask::ALL,
    };
    for z in [3.0, 6.0] {
        world
            .insert_collider(
                None,
                ColliderBuilder::new(shape.clone())
                    .translation(Vec3::new(0.0, 0.0, z))
                    .unwrap()
                    .layers(layers)
                    .surface(SurfaceId(17)),
            )
            .unwrap();
    }
    let ray = RayCast::new(Vec3::ZERO, Vec3::Z, 10.0, true).unwrap();
    assert!(
        world
            .ray_cast(
                ray,
                QueryFilter {
                    layers: LayerMask::from_bits(1 << 1),
                    ..QueryFilter::default()
                }
            )
            .is_none()
    );
    let hit = world
        .ray_cast(
            ray,
            QueryFilter {
                layers: LayerMask::from_bits(1 << 2),
                ..QueryFilter::default()
            },
        )
        .unwrap();
    assert_eq!(hit.surface, SurfaceId(17));
    let body = world.insert_body(BodyBuilder::static_body());
    world
        .insert_collider(
            Some(body),
            ColliderBuilder::new(shape)
                .translation(Vec3::new(0.0, 0.0, 9.0))
                .unwrap(),
        )
        .unwrap();
    let filtered = QueryFilter::default().excluding_body(body);
    assert_eq!(
        world.ray_cast(ray, filtered).unwrap().surface,
        SurfaceId(17)
    );
}

#[test]
fn queries_include_sensors_only_when_requested() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(0.5)).unwrap())
                .translation(Vec3::new(0.0, 0.0, 3.0))
                .unwrap()
                .sensor(true),
        )
        .unwrap();
    let ray = RayCast::new(Vec3::ZERO, Vec3::Z, 10.0, true).unwrap();
    assert!(world.ray_cast(ray, QueryFilter::default()).is_none());
    assert!(
        world
            .ray_cast(
                ray,
                QueryFilter {
                    include_sensors: true,
                    ..QueryFilter::default()
                }
            )
            .is_some()
    );
}

#[test]
fn characters_walk_across_two_and_five_hundred_twelve_triangle_floors() {
    for subdivisions in [1_u32, 16_u32] {
        let mut vertices = Vec::new();
        for z in 0..=subdivisions {
            for x in 0..=subdivisions {
                vertices.push(Vec3::new(
                    -2.0 + 4.0 * x as f32 / subdivisions as f32,
                    0.0,
                    -2.0 + 4.0 * z as f32 / subdivisions as f32,
                ));
            }
        }
        let width = subdivisions + 1;
        let mut triangles = Vec::new();
        for z in 0..subdivisions {
            for x in 0..subdivisions {
                let a = z * width + x;
                let b = a + 1;
                let c = a + width;
                let d = c + 1;
                triangles.push([a, c, b]);
                triangles.push([b, c, d]);
            }
        }
        let mut world = PhysicsWorld::new(PhysicsConfig::default());
        let mesh = TriMeshShape::new(vertices, triangles).unwrap();
        world
            .insert_collider(None, ColliderBuilder::new(ColliderShape::TriMesh(mesh)))
            .unwrap();
        let config = CharacterConfig::capsule(0.5, 0.4).unwrap();
        let controller = CharacterController::new(config);
        let body = world.insert_body(BodyBuilder::kinematic());
        let mut state = CharacterState::with_motion(Vec3::new(-1.5, 0.9, -1.5), 0.0, true).unwrap();
        let input = CharacterInput::new(Vec3::new(1.5, 0.0, 1.5), false).unwrap();
        for _ in 0..90 {
            state = controller.move_character(state, input, &world, body).state;
            assert!(state.grounded());
        }
        assert!(state.position().x > -0.5 && state.position().z > -0.5);
    }
}

#[test]
fn body_ids_are_generation_checked_after_removal() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let first = world.insert_body(BodyBuilder::dynamic());
    world.remove_body(first).unwrap();
    assert_eq!(world.body_state(first), Err(PhysicsError::StaleBody(first)));

    let replacement = world.insert_body(BodyBuilder::dynamic());
    assert_ne!(first, replacement);
}

#[test]
fn fixed_step_advances_dynamic_body_without_a_clock() {
    let config = PhysicsConfig::new(0.25, Vec3::new(0.0, -4.0, 0.0)).unwrap();
    let mut world = PhysicsWorld::new(config);
    let body = world.insert_body(
        BodyBuilder::dynamic()
            .translation(Vec3::new(0.0, 5.0, 0.0))
            .unwrap(),
    );
    world
        .insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::ball(0.5).unwrap()),
        )
        .unwrap();

    let before = world.body_state(body).unwrap();
    world.step();
    let after = world.body_state(body).unwrap();
    assert!(after.translation.y < before.translation.y);
    assert!(after.linear_velocity.y < 0.0);
}

#[test]
fn kinematic_target_rejects_dynamic_body() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let body = world.insert_body(BodyBuilder::dynamic());
    assert!(matches!(
        world.set_next_kinematic_pose(body, Vec3::ONE, Quat::IDENTITY),
        Err(PhysicsError::WrongBodyKind { .. })
    ));
}

#[test]
fn queries_see_collider_removed_and_inserted_between_steps() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let collider = world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap())
                .translation(Vec3::new(0.0, 0.0, 5.0))
                .unwrap(),
        )
        .unwrap();
    let ray = RayCast::new(Vec3::ZERO, Vec3::Z, 10.0, true).unwrap();
    assert!(world.ray_cast(ray, QueryFilter::default()).is_some());

    world.remove_collider(collider).unwrap();
    assert!(world.ray_cast(ray, QueryFilter::default()).is_none());

    let replacement = world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap())
                .translation(Vec3::new(0.0, 0.0, 5.0))
                .unwrap(),
        )
        .unwrap();
    assert_ne!(collider, replacement);
    assert!(world.ray_cast(ray, QueryFilter::default()).is_some());
}

#[test]
fn changing_an_attached_shape_preserves_its_feet_and_a_free_one_changes_in_place() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let body = world.insert_body(
        BodyBuilder::kinematic()
            .translation(Vec3::new(0.0, 0.6, 0.0))
            .unwrap(),
    );
    let collider = world
        .insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::capsule(0.3, 0.3).unwrap()),
        )
        .unwrap();
    let bottom_before = world.body_state(body).unwrap().translation.y - 0.6;
    world
        .set_collider_shape(collider, ConvexShape::capsule(0.6, 0.3).unwrap())
        .unwrap();
    let bottom_after = world.body_state(body).unwrap().translation.y - 0.9;
    assert!((bottom_before - bottom_after).abs() < 1.0e-5);

    let unparented = world
        .insert_collider(None, ColliderBuilder::new(ConvexShape::ball(0.2).unwrap()))
        .unwrap();
    world
        .set_collider_shape(unparented, ConvexShape::ball(0.3).unwrap())
        .unwrap();
    let hit = world
        .ray_cast(
            RayCast::new(Vec3::new(0.0, -5.0, 0.0), Vec3::Y, 10.0, true).unwrap(),
            QueryFilter::default(),
        )
        .unwrap();
    // The free ball stays at the origin and now reaches 0.3 m below it.
    assert!((hit.distance - 4.7).abs() < 1.0e-4);
}

#[test]
fn ray_and_shape_casts_return_typed_owners() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let body = world.insert_body(
        BodyBuilder::static_body()
            .translation(Vec3::new(0.0, 0.0, 5.0))
            .unwrap(),
    );
    let collider = world
        .insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap()),
        )
        .unwrap();

    let ray = RayCast::new(Vec3::ZERO, Vec3::Z, 10.0, true).unwrap();
    let ray_hit = world
        .ray_cast(ray, QueryFilter::default())
        .expect("ray must hit box");
    assert_eq!(ray_hit.owner.collider, collider);
    assert_eq!(ray_hit.owner.body, Some(body));
    assert!((ray_hit.distance - 4.0).abs() < 1.0e-4);

    let cast = ShapeCast::new(
        ConvexShape::ball(0.5).unwrap(),
        Vec3::ZERO,
        Quat::IDENTITY,
        Vec3::Z,
        10.0,
    )
    .unwrap();
    let shape_hit = world
        .shape_cast(cast, QueryFilter::default())
        .expect("shape must hit box");
    assert_eq!(shape_hit.owner.collider, collider);
    assert!((shape_hit.distance - 3.5).abs() < 1.0e-3);
}

#[test]
fn intersection_excluding_a_body_ignores_all_its_colliders_only() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let player = world.insert_body(BodyBuilder::dynamic());
    for half in [Vec3::ONE, Vec3::splat(0.5)] {
        world
            .insert_collider(
                Some(player),
                ColliderBuilder::new(ConvexShape::cuboid(half).unwrap()),
            )
            .unwrap();
    }
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap())
                .translation(Vec3::new(5.0, 0.0, 0.0))
                .unwrap(),
        )
        .unwrap();
    let probe = ConvexShape::ball(0.2).unwrap();

    assert!(
        world
            .intersects_shape(
                probe.clone(),
                Vec3::ZERO,
                Quat::IDENTITY,
                QueryFilter::default()
            )
            .unwrap()
    );
    assert!(
        !world
            .intersects_shape(
                probe.clone(),
                Vec3::ZERO,
                Quat::IDENTITY,
                QueryFilter::default().excluding_body(player)
            )
            .unwrap(),
        "both colliders of the excluded body must be ignored"
    );
    assert!(
        world
            .intersects_shape(
                probe.clone(),
                Vec3::new(5.0, 0.0, 0.0),
                Quat::IDENTITY,
                QueryFilter::default().excluding_body(player)
            )
            .unwrap(),
        "a free collider must still be detected when another body is excluded"
    );

    world.remove_body(player).unwrap();
    assert!(
        !world
            .intersects_shape(
                probe.clone(),
                Vec3::ZERO,
                Quat::IDENTITY,
                QueryFilter::default().excluding_body(player)
            )
            .unwrap()
    );
}

#[test]
fn character_detects_ground_and_jumps() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(10.0, 0.5, 10.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let (controller, body, falling) = spawn_character(
        &mut world,
        CharacterConfig::capsule(0.5, 0.4).unwrap(),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let idle = CharacterInput::new(Vec3::ZERO, false).unwrap();
    let landed = controller.move_character(falling, idle, &world, body).state;
    assert!(landed.grounded());

    let jump = CharacterInput::new(Vec3::ZERO, true).unwrap();
    let jumping = controller.move_character(landed, jump, &world, body).state;
    assert!(jumping.vertical_velocity() > 0.0);
    assert!(jumping.position().y > landed.position().y);
}

#[test]
fn character_snaps_to_ground_within_snap_distance_and_not_beyond() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(10.0, 0.5, 10.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let (controller, body, within) = spawn_character(
        &mut world,
        CharacterConfig::capsule(0.5, 0.4).unwrap(),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let idle = CharacterInput::new(Vec3::ZERO, false).unwrap();

    let snapped = controller.move_character(within, idle, &world, body).state;
    assert!(snapped.grounded());

    let too_high = CharacterState::new(Vec3::new(0.0, 1.5, 0.0)).unwrap();
    let falling = controller
        .move_character(too_high, idle, &world, body)
        .state;
    assert!(!falling.grounded());
}

#[test]
fn character_is_blocked_by_dynamic_box() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();

    const BOX_HALF: f32 = 0.5;
    const BOX_CENTER_X: f32 = 2.0;
    let box_body = world.insert_body(
        BodyBuilder::dynamic()
            .translation(Vec3::new(BOX_CENTER_X, BOX_HALF, 0.0))
            .unwrap(),
    );
    world
        .insert_collider(
            Some(box_body),
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(BOX_HALF)).unwrap()),
        )
        .unwrap();
    for _ in 0..8 {
        world.step();
    }
    let box_before = world.body_state(box_body).unwrap().translation;

    const SKIN_WIDTH: f32 = 0.01;
    const CAPSULE_HALF_HEIGHT: f32 = 0.5;
    const CAPSULE_RADIUS: f32 = 0.4;
    let clearance_shape = ConvexShape::capsule(CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS - SKIN_WIDTH)
        .expect("positive inset radius");
    let (controller, body, mut state) = spawn_character(
        &mut world,
        CharacterConfig::capsule(CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS).unwrap(),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let box_face_x = BOX_CENTER_X - BOX_HALF;
    state = CharacterState::with_motion(state.position(), 0.0, true).unwrap();
    let input = CharacterInput::new(Vec3::new(6.0, 0.0, 0.0), false).unwrap();
    let max_center_x = box_face_x - CAPSULE_RADIUS - SKIN_WIDTH;

    for _ in 0..120 {
        state = controller.move_character(state, input, &world, body).state;
        assert!(
            !world
                .intersects_shape(
                    clearance_shape.clone(),
                    state.position(),
                    Quat::IDENTITY,
                    QueryFilter::default().excluding_body(body)
                )
                .unwrap()
        );
        assert!(state.position().x <= max_center_x + 1.0e-3);
    }
    let box_after = world.body_state(box_body).unwrap().translation;
    assert!((box_after.x - box_before.x).abs() < 0.05);
}

#[test]
fn character_stands_on_dynamic_box() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();

    const BOX_HALF: f32 = 0.5;
    let box_body = world.insert_body(
        BodyBuilder::dynamic()
            .translation(Vec3::new(0.0, BOX_HALF, 0.0))
            .unwrap(),
    );
    world
        .insert_collider(
            Some(box_body),
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(BOX_HALF)).unwrap()),
        )
        .unwrap();
    for _ in 0..8 {
        world.step();
    }
    let box_rest = world.body_state(box_body).unwrap();
    let box_top = box_rest.translation.y + BOX_HALF;
    assert!(
        box_top > 0.25,
        "dynamic box should rest above the floor for this test"
    );

    const SKIN_WIDTH: f32 = 0.01;
    const CAPSULE_HALF_HEIGHT: f32 = 0.5;
    const CAPSULE_RADIUS: f32 = 0.4;
    let (controller, body, mut state) = spawn_character(
        &mut world,
        CharacterConfig::capsule(CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS).unwrap(),
        Vec3::new(0.0, 3.0, 0.0),
    );
    state = CharacterState::with_motion(state.position(), 0.0, false).unwrap();
    let idle = CharacterInput::new(Vec3::ZERO, false).unwrap();
    let feet_offset = CAPSULE_HALF_HEIGHT + CAPSULE_RADIUS;
    let min_feet_on_box = box_top - SKIN_WIDTH;

    for _ in 0..120 {
        state = controller.move_character(state, idle, &world, body).state;
        if state.grounded() {
            break;
        }
    }
    assert!(state.grounded());
    let feet_y = state.position().y - feet_offset;
    assert!(feet_y >= min_feet_on_box - 1.0e-3);
}

#[test]
fn state_hash_changes_with_body_state() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let body = world.insert_body(BodyBuilder::kinematic());
    let before = world.state_hash();
    world
        .set_body_pose(body, Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY)
        .unwrap();
    assert_ne!(before, world.state_hash());
}

fn resource_world_with_falling_ball() -> (World, engine_physics::BodyId) {
    let config = PhysicsConfig::new(0.25, Vec3::new(0.0, -4.0, 0.0)).unwrap();
    let mut physics = PhysicsWorld::new(config);
    let body = physics.insert_body(
        BodyBuilder::dynamic()
            .translation(Vec3::new(0.0, 5.0, 0.0))
            .unwrap(),
    );
    physics
        .insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::ball(0.5).unwrap()),
        )
        .unwrap();
    let mut world = World::new(3).unwrap();
    world.insert_resource(physics).unwrap();
    (world, body)
}

#[test]
fn stepping_the_physics_resource_changes_the_world_state_hash() {
    let (mut world, _body) = resource_world_with_falling_ball();
    let before = world.state_hash().unwrap();
    world.resource_mut::<PhysicsWorld>().unwrap().step();
    assert_ne!(before, world.state_hash().unwrap());
}

#[test]
fn identical_physics_runs_hash_equal_through_the_world() {
    let (mut a, _) = resource_world_with_falling_ball();
    let (mut b, _) = resource_world_with_falling_ball();
    for _ in 0..30 {
        a.resource_mut::<PhysicsWorld>().unwrap().step();
        b.resource_mut::<PhysicsWorld>().unwrap().step();
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
    }
}

#[test]
fn physics_resource_reads_back_after_stepping() {
    let (mut world, body) = resource_world_with_falling_ball();
    world.resource_mut::<PhysicsWorld>().unwrap().step();
    let state = world
        .resource::<PhysicsWorld>()
        .unwrap()
        .body_state(body)
        .unwrap();
    assert!(state.linear_velocity.y < 0.0);
}

/// The character used by the sandbox: capsule 0.8 m tall, step 0.3 m, slope limit 45 degrees.
fn sandbox_character(
    world: &mut PhysicsWorld,
    position: Vec3,
) -> (CharacterController, engine_physics::BodyId) {
    let config = CharacterConfig::capsule(0.5, 0.3)
        .unwrap()
        .skin_width(0.01)
        .unwrap()
        .step_up(0.3, 0.2)
        .unwrap()
        .slope_limit(45.0_f32.to_radians())
        .unwrap()
        .ground_snap_distance(0.15)
        .unwrap();
    let (controller, body, _) = spawn_character(world, config, position);
    (controller, body)
}

/// A 40 m floor plus the given boxes, each as (centre, half extents, rotation).
fn world_with_boxes(boxes: &[(Vec3, Vec3, Quat)]) -> PhysicsWorld {
    let config = PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).unwrap();
    let mut world = PhysicsWorld::new(config);
    let floor = (
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(20.0, 0.5, 20.0),
        Quat::IDENTITY,
    );
    for (centre, half, rotation) in std::iter::once(floor).chain(boxes.iter().copied()) {
        world
            .insert_collider(
                None,
                ColliderBuilder::new(ConvexShape::cuboid(half).unwrap())
                    .translation(centre)
                    .unwrap()
                    .rotation(rotation)
                    .unwrap(),
            )
            .unwrap();
    }
    world
}

fn triangle_mesh_from_quads(quads: &[[Vec3; 4]]) -> TriMeshShape {
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut triangles = Vec::with_capacity(quads.len() * 2);
    for quad in quads {
        let offset = u32::try_from(vertices.len()).unwrap();
        let [a, b, c, d] = *quad;
        vertices.extend([a, b, c, d]);
        triangles.push([offset, offset + 2, offset + 1]);
        triangles.push([offset + 1, offset + 2, offset + 3]);
    }
    TriMeshShape::new(vertices, triangles).unwrap()
}

fn world_with_triangle_mesh(quads: &[[Vec3; 4]]) -> PhysicsWorld {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ColliderShape::TriMesh(triangle_mesh_from_quads(quads))),
        )
        .unwrap();
    world
}

/// A ramp of half length 3 whose lower end's top edge sits on the floor at `start_x`, rising
/// towards +X at `tilt_rad`. Returns (centre, half extents, rotation) for `world_with_boxes`.
fn ramp_box(start_x: f32, tilt_rad: f32) -> (Vec3, Vec3, Quat) {
    let (length, thickness) = (3.0_f32, 0.2_f32);
    let (sin, cos) = tilt_rad.sin_cos();
    let centre = Vec3::new(
        start_x + length * cos + thickness * sin,
        length * sin - thickness * cos,
        0.0,
    );
    (
        centre,
        Vec3::new(length, thickness, 2.0),
        Quat::from_rotation_z(tilt_rad),
    )
}

/// Applies `ground_velocity` for `ticks` fixed ticks and returns the final state.
fn walk_character(
    world: &PhysicsWorld,
    controller: &CharacterController,
    body: engine_physics::BodyId,
    start: CharacterState,
    ground_velocity: Vec3,
    ticks: u32,
) -> CharacterState {
    let input = CharacterInput::new(ground_velocity, false).unwrap();
    let mut state = start;
    for _ in 0..ticks {
        state = controller.move_character(state, input, world, body).state;
    }
    state
}

/// Height of the capsule's lowest point above y = 0 (the floor top).
fn feet(state: CharacterState) -> f32 {
    state.position().y - 0.8
}

#[test]
fn character_climbs_a_quarter_metre_step() {
    let step = (
        Vec3::new(0.0, 0.125, 0.0),
        Vec3::new(1.0, 0.125, 2.0),
        Quat::IDENTITY,
    );
    let mut world = world_with_boxes(&[step]);
    let (controller, body) = sandbox_character(&mut world, Vec3::new(-3.0, 1.0, 0.0));
    let landed = walk_character(
        &world,
        &controller,
        body,
        CharacterState::new(Vec3::new(-3.0, 1.0, 0.0)).unwrap(),
        Vec3::ZERO,
        30,
    );
    // 40 ticks at 4 m/s is 2.67 m: the centre ends at x = -0.33, on top of the step.
    let on_step = walk_character(&world, &controller, body, landed, Vec3::X * 4.0, 40);
    assert!(
        on_step.grounded(),
        "character should be grounded on the step"
    );
    assert!(
        (-1.0..=1.0).contains(&on_step.position().x),
        "character should be over the step, x = {}",
        on_step.position().x
    );
    let height = feet(on_step);
    assert!(
        (height - 0.25).abs() <= 0.03,
        "feet should rest on the 0.25 m step top, got {height} m"
    );
}

#[test]
fn character_climbs_a_twenty_degree_ramp() {
    let mut world = world_with_boxes(&[ramp_box(0.0, 20.0_f32.to_radians())]);
    let (controller, body) = sandbox_character(&mut world, Vec3::new(-2.0, 1.0, 0.0));
    let landed = walk_character(
        &world,
        &controller,
        body,
        CharacterState::new(Vec3::new(-2.0, 1.0, 0.0)).unwrap(),
        Vec3::ZERO,
        30,
    );
    // 90 ticks at 4 m/s is 6 m: the centre ends near x = 4, where the surface is 4 tan 20 = 1.46 m.
    let up = walk_character(&world, &controller, body, landed, Vec3::X * 4.0, 90);
    assert!(
        up.grounded(),
        "character should be grounded on the 20 degree ramp"
    );
    let height = feet(up);
    assert!(
        height > 1.0,
        "walking up a 20 degree ramp should gain more than 1 m, got {height} m at x = {}",
        up.position().x
    );
}

#[test]
fn character_climbs_a_thirty_degree_triangle_mesh_ramp() {
    let rise = 4.0 * 30.0_f32.to_radians().tan();
    let quads = [
        [
            Vec3::new(-20.0, 0.0, -20.0),
            Vec3::new(0.0, 0.0, -20.0),
            Vec3::new(-20.0, 0.0, 20.0),
            Vec3::new(0.0, 0.0, 20.0),
        ],
        [
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(4.0, rise, -2.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(4.0, rise, 2.0),
        ],
    ];
    let mut world = world_with_triangle_mesh(&quads);
    let config = CharacterConfig::capsule(0.5, 0.3)
        .unwrap()
        .step_up(0.3, 0.2)
        .unwrap()
        .slope_limit(45.0_f32.to_radians())
        .unwrap();
    let controller = CharacterController::new(config);
    let body_id = world.insert_body(BodyBuilder::kinematic());
    world
        .insert_collider(
            Some(body_id),
            ColliderBuilder::new(ConvexShape::capsule(0.5, 0.3).unwrap()),
        )
        .unwrap();
    let mut state = CharacterState::with_motion(Vec3::new(-2.0, 0.9, 0.0), 0.0, true).unwrap();
    let input = CharacterInput::new(Vec3::X * 4.0, false).unwrap();
    for _ in 0..90 {
        state = controller
            .move_character(state, input, &world, body_id)
            .state;
    }
    assert!(
        state.grounded(),
        "character should remain grounded on the mesh ramp"
    );
    assert!(
        state.position().y - 0.9 > 1.5,
        "30 degree mesh ramp should raise the character, got y={}",
        state.position().y
    );
}

#[test]
fn character_climbs_eighteen_centimetre_triangle_mesh_stairs() {
    const STEP_DEPTH_M: f32 = 0.6;
    const STEP_HEIGHT_M: f32 = 0.18;
    const STAIR_COUNT: u16 = 8;
    let mut quads = vec![[
        Vec3::new(-20.0, 0.0, -2.0),
        Vec3::new(0.0, 0.0, -2.0),
        Vec3::new(-20.0, 0.0, 2.0),
        Vec3::new(0.0, 0.0, 2.0),
    ]];
    for step in 0..STAIR_COUNT {
        let x = f32::from(step) * STEP_DEPTH_M;
        let lower = f32::from(step) * STEP_HEIGHT_M;
        let upper = lower + STEP_HEIGHT_M;
        quads.push([
            Vec3::new(x, upper, -2.0),
            Vec3::new(x + STEP_DEPTH_M, upper, -2.0),
            Vec3::new(x, upper, 2.0),
            Vec3::new(x + STEP_DEPTH_M, upper, 2.0),
        ]);
        quads.push([
            Vec3::new(x, lower, -2.0),
            Vec3::new(x, upper, -2.0),
            Vec3::new(x, lower, 2.0),
            Vec3::new(x, upper, 2.0),
        ]);
    }
    let mut world = world_with_triangle_mesh(&quads);
    let config = CharacterConfig::capsule(0.5, 0.3)
        .unwrap()
        .step_up(0.3, 0.2)
        .unwrap();
    let controller = CharacterController::new(config);
    let body = world.insert_body(BodyBuilder::kinematic());
    world
        .insert_collider(
            Some(body),
            ColliderBuilder::new(ConvexShape::capsule(0.5, 0.3).unwrap()),
        )
        .unwrap();
    let mut state = CharacterState::with_motion(Vec3::new(-1.5, 0.9, 0.0), 0.0, true).unwrap();
    let input = CharacterInput::new(Vec3::X * 4.0, false).unwrap();
    for _ in 0..65 {
        state = controller.move_character(state, input, &world, body).state;
    }
    assert!(
        state.grounded(),
        "character should be grounded on the mesh stairs"
    );
    let foot_height = feet(state);
    assert!(
        foot_height >= 0.7,
        "character should climb several 18 cm steps, feet={foot_height}"
    );
}

#[test]
fn character_is_blocked_by_a_fifty_degree_ramp() {
    let mut world = world_with_boxes(&[ramp_box(0.0, 50.0_f32.to_radians())]);
    let (controller, body) = sandbox_character(&mut world, Vec3::new(-2.0, 1.0, 0.0));
    let mut state = walk_character(
        &world,
        &controller,
        body,
        CharacterState::new(Vec3::new(-2.0, 1.0, 0.0)).unwrap(),
        Vec3::ZERO,
        30,
    );
    let input = CharacterInput::new(Vec3::X * 4.0, false).unwrap();
    let mut highest = f32::MIN;
    for _ in 0..120 {
        state = controller.move_character(state, input, &world, body).state;
        highest = highest.max(feet(state));
    }
    // Slope limit 45 degrees: the 50 degree face must never raise the character.
    assert!(
        highest < 0.2,
        "a 50 degree face is above the 45 degree limit; feet must stay below 0.2 m, highest {highest} m"
    );
    assert!(
        state.position().x < 0.0,
        "character must stay on the near side, x = {}",
        state.position().x
    );
}

#[test]
fn ray_excluding_a_body_passes_through_its_colliders_only() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    let near = world.insert_body(
        BodyBuilder::static_body()
            .translation(Vec3::new(0.0, 0.0, 2.0))
            .unwrap(),
    );
    world
        .insert_collider(
            Some(near),
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap()),
        )
        .unwrap();
    let far = world.insert_body(
        BodyBuilder::static_body()
            .translation(Vec3::new(0.0, 0.0, 6.0))
            .unwrap(),
    );
    let far_collider = world
        .insert_collider(
            Some(far),
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::ONE).unwrap()),
        )
        .unwrap();
    let ray = RayCast::new(Vec3::ZERO, Vec3::Z, 20.0, true).unwrap();

    let hit = world
        .ray_cast(ray, QueryFilter::default().excluding_body(near))
        .expect("the far box must be hit");
    assert_eq!(hit.owner.collider, far_collider);
    assert_eq!(hit.owner.body, Some(far));

    let hit = world.ray_cast(ray, QueryFilter::default().excluding_body(far));
    assert_eq!(hit.map(|h| h.owner.body), Some(Some(near)));

    world.remove_body(near).unwrap();
    assert!(
        world
            .ray_cast(ray, QueryFilter::default().excluding_body(near))
            .is_some()
    );
}

#[test]
fn character_ignores_its_own_capsule_for_six_hundred_ticks() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(50.0, 0.5, 50.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    let (controller, body, mut state) = spawn_character(
        &mut world,
        CharacterConfig::capsule(0.5, 0.4).unwrap(),
        Vec3::new(0.0, 0.9, 0.0),
    );
    let walk = Vec3::new(3.0, 0.0, 0.0);
    let start_x = state.position().x;
    for _ in 0..600 {
        let input = CharacterInput::new(walk, false).unwrap();
        state = controller.move_character(state, input, &world, body).state;
        world
            .set_next_kinematic_pose(body, state.position(), Quat::IDENTITY)
            .unwrap();
        world.step();
    }
    let walked = state.position().x - start_x;
    assert!(walked > 29.5, "walked {walked} m, expected at least 29.5 m");
    assert!(state.grounded());
}

#[test]
fn character_skin_must_be_thinner_than_the_capsule_radius() {
    assert!(CharacterConfig::capsule(0.5, 0.005).is_err());
    let config = CharacterConfig::capsule(0.5, 0.4).unwrap();
    assert!(config.skin_width(0.4).is_err());
    assert!(config.skin_width(0.39).is_ok());
}

/// Regression for a property-test case: walking into a box tilted 30 degrees over the character
/// wedges the capsule between the floor and the overhang, where Rapier's solve ended 3.7 cm inside
/// the floor. A move that would end embedded is refused, so the capsule never goes deeper than its
/// skin.
#[test]
fn character_wedged_under_an_overhang_never_ends_embedded() {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(20.0, 0.5, 20.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(0.4, 2.0, 3.0)).unwrap())
                .translation(Vec3::new(4.46049, 1.5, 0.0))
                .unwrap()
                .rotation(Quat::from_rotation_z(0.528_900_6))
                .unwrap(),
        )
        .unwrap();
    let config = CharacterConfig::capsule(0.5, 0.4)
        .unwrap()
        .step_up(0.25, 0.2)
        .unwrap()
        .slope_limit(0.7)
        .unwrap();
    let (controller, body, start) = spawn_character(&mut world, config, Vec3::new(0.0, 1.0, 0.0));
    let clearance = ConvexShape::capsule(0.5, 0.4 - 0.01).unwrap();
    let mut state = CharacterState::with_motion(start.position(), 0.0, true).unwrap();
    let input = CharacterInput::new(Vec3::new(4.860_923_3, 0.0, 0.0), false).unwrap();
    for _ in 0..120 {
        state = controller.move_character(state, input, &world, body).state;
        assert!(
            !world
                .intersects_shape(
                    clearance.clone(),
                    state.position(),
                    Quat::IDENTITY,
                    QueryFilter::default().excluding_body(body)
                )
                .unwrap()
        );
    }
    assert!(state.grounded());
}
