//! Full-level record spawning and cooked-scene installation entry points.
use std::collections::{BTreeMap, HashMap};

use engine_assets::format::{AssetId, AssetIndex, MeshPack};
use engine_assets::{AssetPath, ContentPack};
use engine_physics::{ColliderShape, PhysicsWorld};
use engine_world::World;

use crate::error::SceneError;
use crate::model::{CookedColliderDef, Level, ShapeDef};
use crate::prefab::PrefabRegistry;
use crate::visual::Visual;

use super::collider::{spawn_cooked_collider, spawn_prefab_placement};
use super::instance::spawn_cooked_instance;
use super::lifecycle::{despawn, rebuild_entries, spawn_level_with};
use super::primitives::{spawn_shape, spawn_shape_with_metadata};
use super::records::{LevelInstance, Spawned};
use super::types::{ColliderMetadata, Markers, SceneContent};

/// Spawns every shape of a level, in file order. All of them or none: when one fails, the shapes
/// already spawned are removed again.
///
/// # Errors
///
/// Returns the first error from [`spawn_shape`]. The world is then as it was before the call. A
/// failed removal of those shapes is reported instead, which only a broken world can cause.
pub fn spawn_level(world: &mut World, level: &Level) -> Result<Spawned, SceneError> {
    spawn_level_with(world, level, spawn_shape)
}

/// Spawns a level using game-provided layers and surfaces for each greybox shape.
pub fn spawn_level_with_metadata(
    world: &mut World,
    level: &Level,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
) -> Result<Spawned, SceneError> {
    let mut bodies = Vec::with_capacity(level.shapes().len());
    for shape in level.shapes() {
        match spawn_shape_with_metadata(world, shape, metadata(shape, None)) {
            Ok(body) => bodies.push(body),
            Err(error) => {
                despawn(
                    world,
                    &Spawned {
                        entries: BTreeMap::new(),
                        bodies,
                        colliders: Vec::new(),
                        instances: Vec::new(),
                        instance_bodies: Vec::new(),
                        prefabs: Vec::new(),
                        markers: Markers::default(),
                    },
                )?;
                return Err(error);
            }
        }
    }
    let mut spawned = Spawned {
        entries: BTreeMap::new(),
        bodies,
        colliders: Vec::new(),
        instances: Vec::new(),
        instance_bodies: Vec::new(),
        prefabs: Vec::new(),
        markers: Markers::of(level),
    };
    rebuild_entries(&mut spawned, level)?;
    Ok(spawned)
}

/// Spawns a level and resolves cooked collision packs from the mounted game pack.
pub fn spawn_level_from_content(
    world: &mut World,
    level: &Level,
    content: &ContentPack,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
) -> Result<Spawned, SceneError> {
    if !level.prefabs().is_empty() {
        return Err(SceneError::Prefab {
            file: level.name().to_owned(),
            field: "prefabs".to_owned(),
            reason: "this game must supply its PrefabRegistry".to_owned(),
        });
    }
    spawn_level_from_content_with_prefabs(world, level, content, metadata, None)
}

/// Spawns a level from the game-owned canonical scene content.
///
/// This is the normal content path. Compatibility entry points below retain the earlier explicit
/// root, metadata and registry parameters for existing callers.
pub fn spawn_level_from_scene_content(
    world: &mut World,
    level: &Level,
    content: &SceneContent,
) -> Result<Spawned, SceneError> {
    spawn_level_from_content_with_prefabs(
        world,
        level,
        content.pack(),
        |shape, surface| content.collider_metadata(Some(shape), surface),
        Some(content.prefabs()),
    )
}

/// Atomically installs a level from the game-owned canonical scene content.
pub fn install_level_from_scene_content(
    world: &mut World,
    level: &Level,
    content: &SceneContent,
) -> Result<(), SceneError> {
    if world.resource::<LevelInstance>().is_err() {
        world.insert_resource(LevelInstance::default())?;
    }
    let spawned = spawn_level_from_scene_content(world, level, content)?;
    let previous = std::mem::replace(&mut world.resource_mut::<LevelInstance>()?.spawned, spawned);
    despawn(world, &previous)
}

/// Spawns cooked level data and game-registered prefab blocks.
pub fn spawn_level_from_content_with_prefabs(
    world: &mut World,
    level: &Level,
    content: &ContentPack,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
    registry: Option<&PrefabRegistry>,
) -> Result<Spawned, SceneError> {
    if level.colliders().is_empty() && level.instances().is_empty() && level.prefabs().is_empty() {
        return spawn_level_with_metadata(world, level, metadata);
    }
    let index_path =
        AssetPath::new("content/cooked/index.ron").map_err(|error| SceneError::AssetIndex {
            path: "content/cooked/index.ron".into(),
            reason: error.to_string(),
        })?;
    let index_text = content
        .load_text(&index_path)
        .map_err(|error| SceneError::AssetIndex {
            path: index_path.as_str().into(),
            reason: error.to_string(),
        })?;
    let index =
        ron::from_str::<AssetIndex>(index_text).map_err(|error| SceneError::AssetIndex {
            path: index_path.as_str().into(),
            reason: error.to_string(),
        })?;
    let mut spawned = spawn_level_with_metadata(world, level, metadata)?;
    let mut packs = HashMap::<AssetPath, MeshPack>::new();
    let mut collider_shapes = HashMap::<(AssetId, [u32; 3]), Vec<ColliderShape>>::new();
    let mut instances = Vec::with_capacity(level.instances().len());
    let mut instance_bodies = Vec::with_capacity(level.instances().len());
    for definition in level.instances() {
        let transform = level
            .layouts()
            .first()
            .and_then(|layout| layout.mover_poses.get(&definition.asset_id))
            .copied()
            .unwrap_or(definition.transform);
        let result =
            spawn_cooked_instance(world, definition, transform, &index).and_then(|visual| {
                let Some(collision) = definition.collision else {
                    return Ok((visual, None));
                };
                let collider = spawn_cooked_collider(
                    world,
                    &CookedColliderDef {
                        asset_id: definition.asset_id,
                        transform,
                        sensor: collision.sensor,
                    },
                    content,
                    &index,
                    &mut packs,
                    &mut collider_shapes,
                )?;
                let visual_component = *world.get::<Visual>(visual)?;
                world.despawn(visual)?;
                if let Err(error) = world.insert(collider.id, visual_component) {
                    world
                        .resource_mut::<PhysicsWorld>()?
                        .remove_body(collider.body)?;
                    world.despawn(collider.id)?;
                    return Err(error.into());
                }
                Ok((collider.id, Some(collider.body)))
            });
        match result {
            Ok((instance, body)) => {
                instances.push(instance);
                instance_bodies.push(body);
            }
            Err(error) => {
                spawned.instances = instances;
                spawned.instance_bodies = instance_bodies;
                despawn(world, &spawned)?;
                return Err(error);
            }
        }
    }
    spawned.instances = instances;
    spawned.instance_bodies = instance_bodies;
    let mut colliders = Vec::with_capacity(level.colliders().len());
    for definition in level.colliders() {
        let result = spawn_cooked_collider(
            world,
            definition,
            content,
            &index,
            &mut packs,
            &mut collider_shapes,
        );
        match result {
            Ok(collider) => colliders.push(collider),
            Err(error) => {
                spawned.colliders = colliders;
                despawn(world, &spawned)?;
                return Err(error);
            }
        }
    }
    spawned.colliders = colliders;
    let Some(registry) = registry else {
        if !level.prefabs().is_empty() {
            despawn(world, &spawned)?;
            return Err(SceneError::Prefab {
                file: level.name().to_owned(),
                field: "prefabs".to_owned(),
                reason: "this game must supply its PrefabRegistry".to_owned(),
            });
        }
        rebuild_entries(&mut spawned, level)?;
        return Ok(spawned);
    };
    let mut prefab_entities = Vec::with_capacity(level.prefabs().len());
    for placement in level.prefabs() {
        let result = spawn_prefab_placement(
            world,
            placement,
            content,
            &index,
            &mut packs,
            &mut collider_shapes,
            registry,
        );
        match result {
            Ok((entity, collider)) => {
                prefab_entities.push(entity);
                if let Some(collider) = collider {
                    spawned.colliders.push(collider);
                }
            }
            Err(error) => {
                spawned.prefabs = prefab_entities;
                despawn(world, &spawned)?;
                return Err(error);
            }
        }
    }
    spawned.prefabs = prefab_entities;
    rebuild_entries(&mut spawned, level)?;
    Ok(spawned)
}
