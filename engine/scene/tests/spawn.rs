//! Spawning and removing a level: counts, order, components, markers and clean removal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use engine_physics::{PhysicsConfig, PhysicsWorld};
use engine_scene::{
    Level, LevelInstance, MarkerKind, MeshKind, ShapeKind, Visual, despawn, install_level,
    installed, register_components, spawn_level,
};
use engine_world::{Transform, World};
use glam::Vec3;

const LEVEL: &str = r#"(
    version: 1,
    name: "spawn",
    gravity: (0.0, -9.81, 0.0),
    shapes: [
        (
            id: 1,
            name: Some("floor"),
            kind: Cuboid(half: (10.0, 0.5, 10.0)),
            translation: (0.0, -0.5, 0.0),
            body: Static,
            look: (material: Floor),
        ),
        (
            id: 2,
            kind: Sphere(radius: 0.5),
            translation: (2.0, 3.0, 0.0),
            body: Dynamic(mass_kg: 2.0),
            look: (material: Metal),
        ),
        (
            id: 3,
            name: Some("pillar"),
            kind: Capsule(half_height: 0.5, radius: 0.25),
            translation: (-2.0, 1.0, 4.0),
            rotation_deg: (0.0, 90.0, 0.0),
            body: Kinematic,
            look: (material: Custom(colour_hex: 0x336699, roughness: 0.4, metallic: 0.0), tint: Some(0xFF0000)),
        ),
    ],
    spawns: [
        (name: "p1", translation: (0.0, 1.0, 0.0), yaw_deg: 90.0),
        (name: "p2", translation: (4.0, 1.0, 0.0)),
    ],
    waypoints: [(name: "w1", translation: (1.0, 0.0, 1.0))],
    markers: [(name: "crate-spot", translation: (0.0, 0.0, 0.0))],
)"#;

fn world() -> World {
    let mut world = World::new(7).expect("world");
    let config = PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).expect("config");
    world
        .insert_resource(PhysicsWorld::new(config))
        .expect("physics");
    register_components(&mut world).expect("components");
    world
}

fn level() -> Level {
    Level::parse(LEVEL, "spawn.ron").expect("valid level")
}

#[test]
fn one_entity_and_one_body_per_shape_in_file_order() {
    let mut world = world();
    let spawned = spawn_level(&mut world, &level()).expect("spawn");
    assert_eq!(spawned.bodies().len(), 3);
    assert_eq!(spawned.entries().len(), 3);
    assert_eq!(
        spawned
            .entries()
            .get(&engine_assets::format::AssetId::new(1).expect("id")),
        Some(&engine_scene::SpawnedEntry {
            id: spawned.bodies()[0].id,
            body: Some(spawned.bodies()[0].body),
            authored: *world
                .get::<Transform>(spawned.bodies()[0].id)
                .expect("transform"),
        })
    );
    assert_eq!(world.len(), 3);
    let physics = world.resource::<PhysicsWorld>().expect("physics");
    assert_eq!(physics.body_count(), 3);
    assert_eq!(physics.collider_count(), 3);
    let kinds: Vec<ShapeKind> = spawned.bodies().iter().map(|b| b.kind).collect();
    assert!(matches!(kinds[0], ShapeKind::Cuboid { .. }));
    assert!(matches!(kinds[1], ShapeKind::Sphere { .. }));
    assert!(matches!(kinds[2], ShapeKind::Capsule { .. }));
}

#[test]
fn each_entity_carries_its_transform_and_visual() {
    let mut world = world();
    let spawned = spawn_level(&mut world, &level()).expect("spawn");
    let floor = spawned.bodies()[0];
    let transform = world.get::<Transform>(floor.id).expect("transform");
    assert!((transform.translation() - Vec3::new(0.0, -0.5, 0.0)).length() < 1e-6);
    let scale = transform.matrix().to_scale_rotation_translation().0;
    assert!(
        (scale - Vec3::new(20.0, 1.0, 20.0)).length() < 1e-5,
        "{scale}"
    );
    let visual = *world.get::<Visual>(floor.id).expect("visual");
    assert_eq!(visual.mesh, MeshKind::Box);
    let sphere = spawned.bodies()[1];
    assert_eq!(
        world.get::<Visual>(sphere.id).expect("visual").mesh,
        MeshKind::Sphere
    );
    let capsule = spawned.bodies()[2];
    assert_eq!(
        world.get::<Visual>(capsule.id).expect("visual").mesh,
        MeshKind::Capsule
    );
}

#[test]
fn markers_keep_file_order_and_look_up_by_kind_and_name() {
    let mut world = world();
    let spawned = spawn_level(&mut world, &level()).expect("spawn");
    let markers = spawned.markers();
    let names: Vec<&str> = markers
        .list(MarkerKind::Spawn)
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["p1", "p2"]);
    assert_eq!(
        markers.find(MarkerKind::Spawn, "p2").map(|m| m.translation),
        Some([4.0, 1.0, 0.0])
    );
    assert!(markers.find(MarkerKind::Spawn, "w1").is_none());
    assert!(markers.find(MarkerKind::Waypoint, "w1").is_some());
    assert!(markers.find(MarkerKind::Other, "crate-spot").is_some());
    assert!(markers.find(MarkerKind::Other, "missing").is_none());
}

#[test]
fn despawn_removes_every_entity_body_and_collider() {
    let mut world = world();
    let spawned = spawn_level(&mut world, &level()).expect("spawn");
    despawn(&mut world, &spawned).expect("despawn");
    assert_eq!(world.len(), 0);
    let physics = world.resource::<PhysicsWorld>().expect("physics");
    assert_eq!(physics.body_count(), 0);
    assert_eq!(physics.collider_count(), 0);
}

#[test]
fn installing_twice_replaces_rather_than_adds() {
    let mut world = world();
    install_level(&mut world, &level()).expect("first install");
    install_level(&mut world, &level()).expect("second install");
    assert_eq!(world.len(), 3);
    let physics = world.resource::<PhysicsWorld>().expect("physics");
    assert_eq!(physics.body_count(), 3);
    assert_eq!(physics.collider_count(), 3);
    assert_eq!(installed(&world).expect("installed").bodies().len(), 3);
    assert!(world.resource::<LevelInstance>().is_ok());
}

#[test]
fn spawning_without_registered_components_is_a_world_error() {
    let mut world = World::new(9).expect("world");
    let config = PhysicsConfig::new(1.0 / 60.0, Vec3::ZERO).expect("config");
    world
        .insert_resource(PhysicsWorld::new(config))
        .expect("physics");
    assert!(spawn_level(&mut world, &level()).is_err());
}
