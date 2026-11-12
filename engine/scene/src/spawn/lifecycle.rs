//! Atomic level installation, entry movement, and scene teardown.
use engine_assets::ContentPack;
use std::collections::BTreeMap;

use engine_physics::PhysicsWorld;
use engine_world::{Transform, World, WorldError};
use glam::Vec3;

use crate::error::SceneError;
use crate::model::{Level, ShapeDef};
use crate::prefab::PrefabRegistry;

use super::collider::transform_from_level;
use super::level::{
    spawn_level_from_content, spawn_level_from_content_with_prefabs, spawn_level_with_metadata,
};
use super::primitives::{SpawnFn, rotation_of, spawn_shape};
use super::records::{LevelInstance, Spawned, SpawnedEntry};
use super::types::{ColliderMetadata, Markers};

pub(super) fn spawn_level_with(
    world: &mut World,
    level: &Level,
    spawn: SpawnFn,
) -> Result<Spawned, SceneError> {
    let mut bodies = Vec::with_capacity(level.shapes().len());
    for shape in level.shapes() {
        match spawn(world, shape) {
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

pub(super) fn rebuild_entries(spawned: &mut Spawned, level: &Level) -> Result<(), SceneError> {
    spawned.entries.clear();
    for (shape, body) in level.shapes().iter().zip(&spawned.bodies) {
        spawned.entries.insert(
            shape.id(),
            SpawnedEntry {
                id: body.id,
                body: Some(body.body),
                authored: Transform::new(
                    Vec3::from_array(shape.translation()),
                    rotation_of(shape),
                    Vec3::from_array(shape.kind().full_size()),
                )?,
            },
        );
    }
    for ((definition, id), body) in level
        .instances()
        .iter()
        .zip(&spawned.instances)
        .zip(&spawned.instance_bodies)
    {
        spawned.entries.insert(
            definition.asset_id,
            SpawnedEntry {
                id: *id,
                body: *body,
                authored: transform_from_level(definition.transform)?,
            },
        );
    }
    for (definition, collider) in level.colliders().iter().zip(&spawned.colliders) {
        spawned.entries.insert(
            definition.asset_id,
            SpawnedEntry {
                id: collider.id,
                body: Some(collider.body),
                authored: transform_from_level(definition.transform)?,
            },
        );
    }
    for (definition, id) in level.prefabs().iter().zip(&spawned.prefabs) {
        let body = spawned
            .colliders
            .iter()
            .find(|collider| collider.id == *id)
            .map(|collider| collider.body);
        spawned.entries.insert(
            definition.id,
            SpawnedEntry {
                id: *id,
                body,
                authored: transform_from_level(definition.transform)?,
            },
        );
    }
    Ok(())
}

/// Moves one pristine scene entry in both ECS and physics.
///
/// Callers compare gameplay state before this operation. The body pose and entity transform are
/// committed together at the application boundary.
pub fn move_entry(
    world: &mut World,
    entry: SpawnedEntry,
    transform: Transform,
) -> Result<(), SceneError> {
    if let Some(body) = entry.body {
        world.resource_mut::<PhysicsWorld>()?.set_body_pose(
            body,
            transform.translation(),
            transform.rotation(),
        )?;
    }
    *world.get_mut::<Transform>(entry.id)? = transform;
    Ok(())
}

/// Removes one canonical entry and its attached body, if either still exists.
///
/// This is the lifecycle counterpart to [`spawn_record_from_scene_content`].
pub fn despawn_entry(world: &mut World, entry: SpawnedEntry) -> Result<(), SceneError> {
    if world.is_alive(entry.id) {
        world.despawn(entry.id)?;
    }
    if let Some(body) = entry.body {
        world.resource_mut::<PhysicsWorld>()?.remove_body(body)?;
    }
    Ok(())
}

/// Removes everything `spawned` created: each entity and its physics body (with its collider).
///
/// # Errors
///
/// Returns the first world or physics error. Entities already removed stay removed.
pub fn despawn(world: &mut World, spawned: &Spawned) -> Result<(), SceneError> {
    if spawned.entries.is_empty() {
        for entry in &spawned.bodies {
            if world.is_alive(entry.id) {
                world.despawn(entry.id)?;
            }
            world
                .resource_mut::<PhysicsWorld>()?
                .remove_body(entry.body)?;
        }
        for entry in &spawned.colliders {
            if world.is_alive(entry.id) {
                world.despawn(entry.id)?;
            }
            world
                .resource_mut::<PhysicsWorld>()?
                .remove_body(entry.body)?;
        }
        for id in &spawned.instances {
            if world.is_alive(*id) {
                world.despawn(*id)?;
            }
        }
        for id in &spawned.prefabs {
            if world.is_alive(*id) {
                world.despawn(*id)?;
            }
        }
        return Ok(());
    }
    for entry in spawned.entries.values() {
        despawn_entry(world, *entry)?;
    }
    Ok(())
}

/// Replaces the level in the world with `level`. The new level spawns first: if that fails, the
/// world and [`LevelInstance`] are unchanged and the previous level keeps running. If it
/// succeeds, the new set is recorded and the previous level's entities and bodies are removed.
///
/// # Errors
///
/// Returns the error from spawning the new level, with the world unchanged. The first install
/// needs the physics world and the components registered. A failure while removing the previous
/// level comes after the swap; only a broken world can cause it.
pub fn install_level(world: &mut World, level: &Level) -> Result<(), SceneError> {
    install_level_with(world, level, spawn_shape)
}

/// Atomically installs a level using game-provided collider metadata.
pub fn install_level_with_metadata(
    world: &mut World,
    level: &Level,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
) -> Result<(), SceneError> {
    if world.resource::<LevelInstance>().is_err() {
        world.insert_resource(LevelInstance::default())?;
    }
    let spawned = spawn_level_with_metadata(world, level, metadata)?;
    let previous = std::mem::replace(&mut world.resource_mut::<LevelInstance>()?.spawned, spawned);
    despawn(world, &previous)
}

/// Atomically installs a level whose cooked collider assets resolve from the mounted pack.
pub fn install_level_from_content(
    world: &mut World,
    level: &Level,
    content: &ContentPack,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
) -> Result<(), SceneError> {
    if world.resource::<LevelInstance>().is_err() {
        world.insert_resource(LevelInstance::default())?;
    }
    let spawned = spawn_level_from_content(world, level, content, metadata)?;
    let previous = std::mem::replace(&mut world.resource_mut::<LevelInstance>()?.spawned, spawned);
    despawn(world, &previous)
}

/// Atomically installs a cooked level using the game's registered prefab component blocks.
pub fn install_level_from_content_with_prefabs(
    world: &mut World,
    level: &Level,
    content: &ContentPack,
    metadata: impl Fn(&ShapeDef, Option<&str>) -> ColliderMetadata,
    registry: &PrefabRegistry,
) -> Result<(), SceneError> {
    if world.resource::<LevelInstance>().is_err() {
        world.insert_resource(LevelInstance::default())?;
    }
    let spawned =
        spawn_level_from_content_with_prefabs(world, level, content, metadata, Some(registry))?;
    let previous = std::mem::replace(&mut world.resource_mut::<LevelInstance>()?.spawned, spawned);
    despawn(world, &previous)
}

pub(super) fn install_level_with(
    world: &mut World,
    level: &Level,
    spawn: SpawnFn,
) -> Result<(), SceneError> {
    if world.resource::<LevelInstance>().is_err() {
        world.insert_resource(LevelInstance::default())?;
    }
    let spawned = spawn_level_with(world, level, spawn)?;
    let previous = std::mem::replace(&mut world.resource_mut::<LevelInstance>()?.spawned, spawned);
    despawn(world, &previous)
}

/// The spawned set of the level currently installed.
///
/// # Errors
///
/// Returns a world error when no level has been installed.
pub fn installed(world: &World) -> Result<&Spawned, WorldError> {
    Ok(&world.resource::<LevelInstance>()?.spawned)
}

/// The mutable canonical spawned set of the level currently installed.
///
/// # Errors
///
/// Returns a world error when no level has been installed.
pub fn installed_mut(world: &mut World) -> Result<&mut Spawned, WorldError> {
    Ok(&mut world.resource_mut::<LevelInstance>()?.spawned)
}
