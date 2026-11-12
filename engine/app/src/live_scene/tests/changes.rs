//! Applying stable-ID edits to a running scene.
use super::*;

#[test]
fn live_diff_moves_adds_removes_and_replaces_pristine_shapes() {
    let previous = Level::parse(
            "(version: 1, name: \"a\", gravity: (0.0, -9.81, 0.0), shapes: [(id: 1, kind: Cuboid(half: (1.0, 1.0, 1.0)), translation: (0.0, 0.0, 0.0), body: Static, look: (material: Wall)), (id: 2, kind: Sphere(radius: 1.0), translation: (2.0, 0.0, 0.0), body: Static, look: (material: Wall)), (id: 3, kind: Sphere(radius: 1.0), translation: (3.0, 0.0, 0.0), body: Static, look: (material: Wall))])",
            "previous.ron",
        ).expect("previous level");
    let next = Level::parse(
            "(version: 1, name: \"b\", gravity: (0.0, -9.81, 0.0), shapes: [(id: 1, kind: Cuboid(half: (1.0, 1.0, 1.0)), translation: (1.0, 0.0, 0.0), body: Static, look: (material: Wall)), (id: 3, kind: Sphere(radius: 1.0), translation: (3.0, 0.0, 0.0), body: Static, look: (material: Metal)), (id: 4, kind: Sphere(radius: 1.0), translation: (4.0, 0.0, 0.0), body: Static, look: (material: Wall))])",
            "next.ron",
        ).expect("next level");
    let mut world = World::new(1).expect("world");
    world
        .insert_resource(PhysicsWorld::new(
            PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).expect("physics config"),
        ))
        .expect("physics");
    engine_scene::register_components(&mut world).expect("components");
    engine_scene::install_level(&mut world, &previous).expect("install");
    let old_replaced = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(3).expect("id"))
        .expect("entry")
        .id;

    apply_level_diff(
        &mut world,
        &diff_levels(&previous, &next),
        &next,
        &engine_scene::SceneContent::empty(),
    )
    .expect("apply");

    let spawned = engine_scene::installed(&world).expect("installed");
    assert_eq!(spawned.entries().len(), 3);
    assert!(
        !spawned
            .entries()
            .contains_key(&AssetId::new(2).expect("id"))
    );
    let moved = spawned
        .entries()
        .get(&AssetId::new(1).expect("id"))
        .expect("moved");
    assert_eq!(
        world
            .get::<engine_world::Transform>(moved.id)
            .expect("transform")
            .translation(),
        Vec3::X
    );
    assert_ne!(
        spawned
            .entries()
            .get(&AssetId::new(3).expect("id"))
            .expect("replaced")
            .id,
        old_replaced
    );
    assert_eq!(spawned.bodies().len(), next.shapes().len());
    for (shape, view) in next.shapes().iter().zip(spawned.bodies()) {
        let entry = spawned
            .entries()
            .get(&shape.id())
            .expect("view has canonical entry");
        assert_eq!(view.id, entry.id);
        assert_eq!(Some(view.body), entry.body);
        assert_eq!(view.kind, shape.kind());
        assert_eq!(view.look, shape.look());
    }
    let teardown = spawned.clone();
    engine_scene::despawn(&mut world, &teardown).expect("teardown");
    assert_eq!(world.len(), 0, "live entries left after teardown");
    assert_eq!(
        world
            .resource::<PhysicsWorld>()
            .expect("physics")
            .body_count(),
        0,
        "live bodies left after teardown"
    );
}

#[test]
fn live_diff_replaces_cooked_instance_and_prefab_blocks() {
    let content = prefab_content();
    let previous = Level::parse(
            "(version: 1, name: \"previous\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: 1, transform: (translation: (1.0, 2.0, 3.0)), material_override: Some(0), collision: Some((sensor: false)))], colliders: [(asset_id: 4, transform: (translation: (2.0, 3.0, 4.0)), sensor: false)], prefabs: [(id: 3, prefab_id: 2, transform: (translation: (4.0, 5.0, 6.0)), overrides: {\"door\": {\"open_s\": 0.5}})])",
            "previous.ron",
        )
        .expect("previous level");
    let next = Level::parse(
            "(version: 1, name: \"next\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: 1, transform: (translation: (1.0, 2.0, 3.0)), material_override: Some(1), collision: Some((sensor: false)))], colliders: [(asset_id: 4, transform: (translation: (2.0, 3.0, 4.0)), sensor: true)], prefabs: [(id: 3, prefab_id: 2, transform: (translation: (4.0, 5.0, 6.0)), overrides: {\"door\": {\"open_s\": 0.9}})])",
            "next.ron",
        )
        .expect("next level");
    let mut world = World::new(1).expect("world");
    world
        .insert_resource(PhysicsWorld::new(
            PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).expect("physics config"),
        ))
        .expect("physics");
    engine_scene::register_components(&mut world).expect("scene components");
    world
        .register::<DoorBlock>(Replication::ServerOnly)
        .expect("door component");
    install_level_from_scene_content(&mut world, &previous, &content).expect("install");
    let old_instance = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(1).expect("instance id"))
        .expect("instance entry")
        .id;
    let old_prefab = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(3).expect("placement id"))
        .expect("prefab entry")
        .id;
    let old_collider = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(4).expect("collider id"))
        .expect("collider entry")
        .id;
    assert_eq!(
        world
            .get::<DoorBlock>(old_prefab)
            .expect("initial block")
            .open_s,
        0.5
    );

    let diff = diff_levels(&previous, &next);
    assert_eq!(diff.replaced.len(), 3, "all authored records replace");
    apply_level_diff(&mut world, &diff, &next, &content).expect("apply replacement");

    let entries = engine_scene::installed(&world)
        .expect("installed")
        .entries();
    let instance = entries
        .get(&AssetId::new(1).expect("instance id"))
        .expect("replaced instance")
        .id;
    let prefab = entries
        .get(&AssetId::new(3).expect("placement id"))
        .expect("replaced prefab")
        .id;
    let collider = entries
        .get(&AssetId::new(4).expect("collider id"))
        .expect("replaced collider")
        .id;
    assert_ne!(instance, old_instance, "instance entity was replaced");
    assert_ne!(prefab, old_prefab, "prefab entity was replaced");
    assert_ne!(collider, old_collider, "collider entity was replaced");
    let spawned = engine_scene::installed(&world).expect("installed");
    assert_eq!(spawned.instances(), [instance]);
    assert_eq!(
        spawned.instance_bodies(),
        [entries
            .get(&AssetId::new(1).expect("instance id"))
            .expect("instance entry")
            .body]
    );
    assert_eq!(spawned.prefabs(), [prefab]);
    assert_eq!(spawned.colliders().len(), 2);
    for (view, source_id) in spawned.colliders().iter().zip([4_u64, 3]) {
        let entry = entries
            .get(&AssetId::new(source_id).expect("source id"))
            .expect("collider view has canonical entry");
        assert_eq!(view.id, entry.id);
        assert_eq!(Some(view.body), entry.body);
    }
    assert!(!world.is_alive(old_instance));
    assert!(!world.is_alive(old_prefab));
    assert!(!world.is_alive(old_collider));
    assert!(matches!(
        world.get::<engine_scene::Visual>(instance).expect("instance visual").mesh,
        engine_scene::MeshKind::Cooked(id) if id == AssetId::new(1).expect("model id")
    ));
    assert_eq!(
        world
            .get::<DoorBlock>(prefab)
            .expect("replaced block")
            .open_s,
        0.9
    );
    assert_eq!(world.len(), 3, "old records were removed");
    let teardown = engine_scene::installed(&world).expect("installed").clone();
    engine_scene::despawn(&mut world, &teardown).expect("teardown");
    assert_eq!(world.len(), 0, "live entries left after teardown");
    assert_eq!(
        world
            .resource::<PhysicsWorld>()
            .expect("physics")
            .body_count(),
        0,
        "live bodies left after teardown"
    );
}

#[test]
fn live_diff_skips_gameplay_moved_and_destroyed_entries() {
    let previous = Level::parse(
            "(version: 1, name: \"a\", gravity: (0.0, -9.81, 0.0), shapes: [(id: 1, kind: Sphere(radius: 1.0), translation: (0.0, 0.0, 0.0), body: Static, look: (material: Wall)), (id: 2, kind: Sphere(radius: 1.0), translation: (2.0, 0.0, 0.0), body: Static, look: (material: Wall))])",
            "previous.ron",
        ).expect("previous level");
    let next = Level::parse(
            "(version: 1, name: \"b\", gravity: (0.0, -9.81, 0.0), shapes: [(id: 1, kind: Sphere(radius: 1.0), translation: (1.0, 0.0, 0.0), body: Static, look: (material: Wall))])",
            "next.ron",
        ).expect("next level");
    let mut world = World::new(1).expect("world");
    world
        .insert_resource(PhysicsWorld::new(
            PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0)).expect("physics config"),
        ))
        .expect("physics");
    engine_scene::register_components(&mut world).expect("components");
    engine_scene::install_level(&mut world, &previous).expect("install");
    let moved = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(1).expect("id"))
        .expect("entry")
        .id;
    world
        .get_mut::<engine_world::Transform>(moved)
        .expect("transform")
        .set_translation(Vec3::new(9.0, 0.0, 0.0))
        .expect("move");
    let destroyed = engine_scene::installed(&world)
        .expect("installed")
        .entries()
        .get(&AssetId::new(2).expect("id"))
        .expect("entry")
        .id;
    world.despawn(destroyed).expect("destroy");

    apply_level_diff(
        &mut world,
        &diff_levels(&previous, &next),
        &next,
        &engine_scene::SceneContent::empty(),
    )
    .expect("apply");

    assert_eq!(
        world
            .get::<engine_world::Transform>(moved)
            .expect("transform")
            .translation(),
        Vec3::new(9.0, 0.0, 0.0)
    );
    assert!(
        !engine_scene::installed(&world)
            .expect("installed")
            .entries()
            .contains_key(&AssetId::new(2).expect("id"))
    );
}
