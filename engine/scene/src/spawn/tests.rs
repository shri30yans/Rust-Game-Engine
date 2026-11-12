//! Spawn, rollback, and installed-level lifecycle tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use super::*;
use crate::{Level, MeshKind, SceneError, ShapeDef, Visual};
use engine_assets::format::{
    Asset, AssetId, AssetIndex, AssetLocation, CollisionShape, MeshPack, Node,
};
use engine_assets::{AssetPath, ContentPack, PackReader};
use engine_physics::{PhysicsConfig, PhysicsWorld};
use engine_world::{NetId, World, WorldError};
use glam::Vec3;
use std::io::Cursor;

use super::lifecycle::{install_level_with, spawn_level_with};

/// A world with a physics world and the scene components, as a game's setup makes it.
fn scene_world() -> World {
    let mut world = bare_world();
    register_components(&mut world).expect("components");
    world
}

/// A world with a physics world only: the scene components are not registered.
fn bare_world() -> World {
    let mut world = World::new(5).expect("world");
    let config = PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).expect("config");
    world
        .insert_resource(PhysicsWorld::new(config))
        .expect("physics");
    world
}

fn body_count(world: &World) -> usize {
    world
        .resource::<PhysicsWorld>()
        .expect("physics")
        .body_count()
}

fn cooked_level(asset_id: u64) -> Level {
    let text = format!(
        "(version: 1, name: \"cooked\", gravity: (0.0, -9.81, 0.0), colliders: [(asset_id: {asset_id}, transform: (translation: (0.0, 0.0, 0.0)))])"
    );
    Level::parse(&text, "cooked.level.ron").expect("valid cooked level")
}

fn cooked_instance_level(asset_id: u64) -> Level {
    let text = format!(
        "(version: 1, name: \"instance\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: {asset_id}, transform: (translation: (1.0, 2.0, 3.0)))])"
    );
    Level::parse(&text, "instance.level.ron").expect("valid instance level")
}

fn cooked_instance_level_with_collision(asset_id: u64) -> Level {
    let text = format!(
        "(version: 1, name: \"instance\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: {asset_id}, transform: (translation: (1.0, 2.0, 3.0)), collision: Some((sensor: false)))])"
    );
    Level::parse(&text, "instance.level.ron").expect("valid cooked level")
}

fn cooked_content_pack(id: u64) -> ContentPack {
    let pack = MeshPack {
        source_hash: 7,
        meshes: Vec::new(),
        materials: Vec::new(),
        nodes: vec![Node {
            name: "object".to_owned(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: vec![("asset_id".to_owned(), id.to_string())],
        }],
        clips: Vec::new(),
        collision: vec![engine_assets::format::CollisionSet {
            node: 0,
            surface: "concrete".to_owned(),
            is_sensor: false,
            shapes: vec![CollisionShape::Box {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                half_extents: [1.0; 3],
            }],
        }],
        textures: Vec::new(),
    };
    let bytes = engine_assets::format::encode(&Asset::MeshPack(pack), 7).expect("encode pack");
    let index = AssetIndex::from([(
        AssetId::new(id).expect("asset id"),
        AssetLocation {
            pack: "arena.meshpack".to_owned(),
            node: Some(0),
            content_hash: 7,
        },
    )]);
    let entries = [
        (
            AssetPath::new("content/cooked/arena.meshpack").expect("mesh path"),
            bytes,
        ),
        (
            AssetPath::new("content/cooked/index.ron").expect("index path"),
            ron::to_string(&index).expect("encode index").into_bytes(),
        ),
    ];
    let mut bytes = Vec::new();
    ContentPack::write(entries, &mut bytes).expect("encode content pack");
    PackReader::read(Cursor::new(bytes)).expect("read content pack")
}

/// Three boxes side by side. The middle one is named `boom` so a hook can fail on it.
fn three_boxes() -> Level {
    let shape = |name: &str, x: f32| {
        let id = name.bytes().fold(1_u64, |hash, byte| {
            hash.wrapping_mul(16777619).wrapping_add(u64::from(byte))
        });
        format!(
            "(id: {id}, name: Some(\"{name}\"), kind: Cuboid(half: (1.0, 1.0, 1.0)), translation: ({x}, 0.0, 0.0), body: Static, look: (material: Wall)),"
        )
    };
    let shapes = [shape("a", 0.0), shape("boom", 3.0), shape("c", 6.0)].join("\n");
    let text =
        format!("(version: 1, name: \"three\", gravity: (0.0, -9.81, 0.0), shapes: [{shapes}])");
    Level::parse(&text, "three.ron").expect("valid level")
}

/// Spawns every shape like [`spawn_shape`], but refuses the one named `boom`.
fn fail_on_boom(world: &mut World, shape: &ShapeDef) -> Result<SpawnedBody, SceneError> {
    if shape.name() == Some("boom") {
        return Err(SceneError::World(WorldError::Invariant(
            "injected spawn failure",
        )));
    }
    spawn_shape(world, shape)
}

#[test]
fn a_shape_that_cannot_be_attached_leaves_no_entity_or_body() {
    let mut world = bare_world();
    let level = three_boxes();
    let shape = level.shapes().first().expect("first shape");
    // The visual cannot be attached without its registered component, after the entity
    // exists, so the spawn must remove the entity and the body it made.
    assert!(spawn_shape(&mut world, shape).is_err());
    assert_eq!(world.len(), 0, "entities left behind");
    assert_eq!(body_count(&world), 0, "bodies left behind");
}

#[test]
fn a_level_that_fails_part_way_spawns_nothing() {
    let mut world = scene_world();
    let result = spawn_level_with(&mut world, &three_boxes(), fail_on_boom);
    assert!(result.is_err());
    assert_eq!(world.len(), 0, "entities left behind");
    assert_eq!(body_count(&world), 0, "bodies left behind");
}

#[test]
fn a_failed_install_keeps_the_running_level_untouched() {
    let mut world = scene_world();
    install_level(&mut world, &three_boxes()).expect("first install");
    let ids = |world: &World| -> Vec<NetId> {
        installed(world)
            .expect("installed")
            .bodies()
            .iter()
            .map(|entry| entry.id)
            .collect()
    };
    let running = ids(&world);
    let (entities, bodies) = (world.len(), body_count(&world));
    assert_eq!(bodies, 3);

    let result = install_level_with(&mut world, &three_boxes(), fail_on_boom);

    assert!(result.is_err());
    assert_eq!(world.len(), entities, "entities changed");
    assert_eq!(body_count(&world), bodies, "bodies changed");
    assert_eq!(ids(&world), running, "the recorded level changed");
}

#[test]
fn a_successful_install_replaces_the_previous_level() {
    let mut world = scene_world();
    install_level(&mut world, &three_boxes()).expect("first install");
    install_level(&mut world, &three_boxes()).expect("second install");
    assert_eq!(world.len(), 3, "old level not removed");
    assert_eq!(body_count(&world), 3, "old bodies not removed");
}

#[test]
fn cooked_colliders_resolve_the_asset_id_through_the_index() {
    let content = cooked_content_pack(41);
    let mut world = scene_world();
    let spawned = spawn_level_from_content(&mut world, &cooked_level(41), &content, |_, _| {
        ColliderMetadata::default()
    })
    .expect("stable asset id resolves");
    assert_eq!(spawned.colliders().len(), 1);
    assert_eq!(body_count(&world), 1);
    despawn(&mut world, &spawned).expect("despawn cooked collider");
    assert_eq!(body_count(&world), 0);
}

#[test]
fn a_missing_cooked_collider_asset_id_is_reported() {
    let content = cooked_content_pack(41);
    let mut world = scene_world();
    let result = spawn_level_from_content(&mut world, &cooked_level(42), &content, |_, _| {
        ColliderMetadata::default()
    });
    assert!(
        matches!(result, Err(SceneError::CookedAssetReference { asset_id, .. }) if asset_id.get() == 42)
    );
    assert_eq!(body_count(&world), 0);
}

#[test]
fn a_cooked_instance_spawns_and_despawns_by_stable_id() {
    let content = cooked_content_pack(41);
    let mut world = scene_world();
    let spawned =
        spawn_level_from_content(&mut world, &cooked_instance_level(41), &content, |_, _| {
            ColliderMetadata::default()
        })
        .expect("stable asset id resolves");
    assert_eq!(spawned.instances().len(), 1);
    assert_eq!(body_count(&world), 0);
    let entity = spawned.instances()[0];
    assert!(
        matches!(world.get::<Visual>(entity).expect("visual").mesh, MeshKind::Cooked(id) if id.get() == 41)
    );
    despawn(&mut world, &spawned).expect("despawn instance");
    assert_eq!(world.len(), 0);
}

#[test]
fn a_cooked_visual_and_collision_share_one_entity_and_body() {
    let content = cooked_content_pack(41);
    let mut world = scene_world();
    let spawned = spawn_level_from_content(
        &mut world,
        &cooked_instance_level_with_collision(41),
        &content,
        |_, _| ColliderMetadata::default(),
    )
    .expect("spawn cooked record");
    let entry = spawned
        .entries()
        .get(&AssetId::new(41).expect("id"))
        .expect("entry");
    assert_eq!(world.len(), 1);
    assert!(entry.body.is_some());
    assert!(world.get::<Visual>(entry.id).is_ok());
    despawn(&mut world, &spawned).expect("despawn");
    assert_eq!(world.len(), 0);
    assert_eq!(body_count(&world), 0);
}
