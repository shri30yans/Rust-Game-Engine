//! Canonical single-record scene spawning and cooked index loading.
use std::collections::HashMap;

use engine_assets::format::AssetIndex;
use engine_assets::{AssetPath, ContentPack};
use engine_physics::PhysicsWorld;
use engine_world::{Transform, World};
use glam::Vec3;

use crate::error::SceneError;
use crate::model::CookedColliderDef;
use crate::visual::Visual;

use super::collider::{spawn_cooked_collider, spawn_prefab_placement, transform_from_level};
use super::instance::spawn_cooked_instance;
use super::primitives::{rotation_of, spawn_shape_with_metadata};
use super::records::SpawnedEntry;
use super::types::SceneContent;

/// Converts an authored record placement to its runtime entity transform.
///
/// Shape ramps include their geometry tilt, so callers must use this rather than reconstructing a
/// transform from [`LevelRecordDef::transform`].
pub fn authored_transform(record: &crate::LevelRecordDef) -> Result<Transform, SceneError> {
    match record {
        crate::LevelRecordDef::Shape(shape) => Transform::new(
            Vec3::from_array(shape.translation()),
            rotation_of(shape),
            Vec3::from_array(shape.kind().full_size()),
        )
        .map_err(SceneError::from),
        crate::LevelRecordDef::Instance(record) => transform_from_level(record.transform),
        crate::LevelRecordDef::Collider(record) => transform_from_level(record.transform),
        crate::LevelRecordDef::Prefab(record) => transform_from_level(record.transform),
    }
}

/// Spawns one authored record through the same canonical content path used by full installation.
///
/// The returned entry is not installed into [`Spawned`] automatically; callers insert it only
/// after the surrounding replacement boundary has succeeded.
pub fn spawn_record_from_scene_content(
    world: &mut World,
    record: &crate::LevelRecordDef,
    content: &SceneContent,
) -> Result<SpawnedEntry, SceneError> {
    match record {
        crate::LevelRecordDef::Shape(shape) => {
            let spawned = spawn_shape_with_metadata(
                world,
                shape,
                content.collider_metadata(Some(shape), None),
            )?;
            Ok(SpawnedEntry {
                id: spawned.id,
                body: Some(spawned.body),
                authored: Transform::new(
                    Vec3::from_array(shape.translation()),
                    rotation_of(shape),
                    Vec3::from_array(shape.kind().full_size()),
                )?,
            })
        }
        crate::LevelRecordDef::Instance(definition) => {
            let index = load_asset_index(content.pack())?;
            let mut packs = HashMap::new();
            let mut shapes = HashMap::new();
            let visual = spawn_cooked_instance(world, definition, definition.transform, &index)?;
            let Some(collision) = definition.collision else {
                return Ok(SpawnedEntry {
                    id: visual,
                    body: None,
                    authored: transform_from_level(definition.transform)?,
                });
            };
            let collider = spawn_cooked_collider(
                world,
                &CookedColliderDef {
                    asset_id: definition.asset_id,
                    transform: definition.transform,
                    sensor: collision.sensor,
                },
                content.pack(),
                &index,
                &mut packs,
                &mut shapes,
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
            Ok(SpawnedEntry {
                id: collider.id,
                body: Some(collider.body),
                authored: transform_from_level(definition.transform)?,
            })
        }
        crate::LevelRecordDef::Collider(definition) => {
            let index = load_asset_index(content.pack())?;
            let mut packs = HashMap::new();
            let mut shapes = HashMap::new();
            let collider = spawn_cooked_collider(
                world,
                definition,
                content.pack(),
                &index,
                &mut packs,
                &mut shapes,
            )?;
            Ok(SpawnedEntry {
                id: collider.id,
                body: Some(collider.body),
                authored: transform_from_level(definition.transform)?,
            })
        }
        crate::LevelRecordDef::Prefab(placement) => {
            let index = load_asset_index(content.pack())?;
            let mut packs = HashMap::new();
            let mut shapes = HashMap::new();
            let (id, collider) = spawn_prefab_placement(
                world,
                placement,
                content.pack(),
                &index,
                &mut packs,
                &mut shapes,
                content.prefabs(),
            )?;
            Ok(SpawnedEntry {
                id,
                body: collider.map(|entry| entry.body),
                authored: transform_from_level(placement.transform)?,
            })
        }
    }
}

pub(super) fn load_asset_index(content: &ContentPack) -> Result<AssetIndex, SceneError> {
    let path =
        AssetPath::new("content/cooked/index.ron").map_err(|error| SceneError::AssetIndex {
            path: "content/cooked/index.ron".into(),
            reason: error.to_string(),
        })?;
    let text = content
        .load_text(&path)
        .map_err(|error| SceneError::AssetIndex {
            path: path.as_str().into(),
            reason: error.to_string(),
        })?;
    ron::from_str(text).map_err(|error| SceneError::AssetIndex {
        path: path.as_str().into(),
        reason: error.to_string(),
    })
}
