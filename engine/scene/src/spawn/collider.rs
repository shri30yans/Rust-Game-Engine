//! Cooked collision and prefab construction from validated content references.
use std::collections::HashMap;
use std::path::Path;

use engine_assets::format::{AssetId, AssetIndex, CollisionShape, MeshPack};
use engine_assets::{AssetPath, ContentPack};
use engine_physics::{
    BodyBuilder, ColliderBuilder, ColliderShape, CollisionLayers, PhysicsWorld, SurfaceId,
};
use engine_world::{NetId, Transform, World};
use glam::{EulerRot, Quat, Vec3};

use crate::error::SceneError;
use crate::model::{CookedColliderDef, LevelTransform, Look, MaterialKind, PrefabPlacement};
use crate::prefab::{PrefabCollision, PrefabRegistry, PrefabSpawn, parse_prefab};
use crate::visual::{MeshKind, Visual};

use super::SpawnedCollider;

pub(super) fn spawn_cooked_collider(
    world: &mut World,
    definition: &CookedColliderDef,
    content: &ContentPack,
    index: &AssetIndex,
    packs: &mut HashMap<AssetPath, MeshPack>,
    shape_cache: &mut HashMap<(AssetId, [u32; 3]), Vec<ColliderShape>>,
) -> Result<SpawnedCollider, SceneError> {
    let asset_id = definition.asset_id;
    let location = index
        .get(&asset_id)
        .ok_or_else(|| SceneError::CookedAssetReference {
            asset_id,
            reason: "asset ID is not present in cooked/index.ron".to_owned(),
        })?;
    let node = location
        .node
        .ok_or_else(|| SceneError::CookedAssetReference {
            asset_id,
            reason: "asset ID names a source file, not an object node".to_owned(),
        })?;
    let relative_path = Path::new(&location.pack);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(SceneError::CookedAssetReference {
            asset_id,
            reason: "index pack path must stay inside the cooked directory".to_owned(),
        });
    }
    let pack_path = cooked_pack_path(asset_id, relative_path)?;
    if !packs.contains_key(&pack_path) {
        let bytes = content
            .load(&pack_path)
            .map_err(|error| SceneError::CookedAssetReference {
                asset_id,
                reason: error.to_string(),
            })?;
        let pack = engine_assets::load::load_mesh_pack(bytes).map_err(|error| {
            SceneError::CookedAssetReference {
                asset_id,
                reason: error.to_string(),
            }
        })?;
        packs.insert(pack_path.clone(), pack);
    }
    let pack = packs
        .get(&pack_path)
        .ok_or_else(|| SceneError::CookedAssetReference {
            asset_id,
            reason: "cooked pack cache entry is missing".to_owned(),
        })?;
    let set = pack
        .collision
        .iter()
        .find(|set| set.node == node)
        .ok_or_else(|| SceneError::CookedAssetReference {
            asset_id,
            reason: "node has no cooked collision set".to_owned(),
        })?;
    let cache_key = (asset_id, definition.transform.scale.map(f32::to_bits));
    if let std::collections::hash_map::Entry::Vacant(entry) = shape_cache.entry(cache_key) {
        let mut shapes = Vec::with_capacity(set.shapes.len());
        for shape in &set.shapes {
            let cooked = scale_cooked_shape(shape, Vec3::from_array(definition.transform.scale));
            let runtime = ColliderShape::from_cooked(&cooked).map_err(|error| {
                SceneError::CookedAssetReference {
                    asset_id,
                    reason: error.to_string(),
                }
            })?;
            shapes.push(runtime);
        }
        entry.insert(shapes);
    }
    let shapes = shape_cache
        .get(&cache_key)
        .ok_or_else(|| SceneError::CookedAssetReference {
            asset_id,
            reason: "cooked collider shape cache entry is missing".to_owned(),
        })?;
    let mut builders = Vec::with_capacity(shapes.len());
    for shape in shapes {
        let builder = ColliderBuilder::new(shape.clone())
            .layers(CollisionLayers::ALL)
            .surface(SurfaceId::DEFAULT)
            .sensor(definition.sensor || set.is_sensor);
        builders.push(builder);
    }
    let [yaw, pitch, roll] = definition.transform.rotation_deg.map(f32::to_radians);
    let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    let translation = Vec3::from_array(definition.transform.translation);
    let scale = Vec3::from_array(definition.transform.scale);
    let transform = Transform::new(translation, rotation, scale)?;
    let body_builder = BodyBuilder::static_body()
        .translation(translation)?
        .rotation(rotation)?;
    world.resource::<PhysicsWorld>()?;
    let id = world.spawn()?;
    if let Err(error) = world.insert(id, transform) {
        world.despawn(id)?;
        return Err(error.into());
    }
    let physics = world.resource_mut::<PhysicsWorld>()?;
    let body = physics.insert_body(body_builder);
    for builder in builders {
        if let Err(error) = physics.insert_collider(Some(body), builder) {
            physics.remove_body(body)?;
            world.despawn(id)?;
            return Err(error.into());
        }
    }
    Ok(SpawnedCollider { id, body })
}

pub(super) fn spawn_prefab_placement(
    world: &mut World,
    placement: &PrefabPlacement,
    content: &ContentPack,
    index: &AssetIndex,
    packs: &mut HashMap<AssetPath, MeshPack>,
    shape_cache: &mut HashMap<(AssetId, [u32; 3]), Vec<ColliderShape>>,
    registry: &PrefabRegistry,
) -> Result<(NetId, Option<SpawnedCollider>), SceneError> {
    let prefab_location =
        index
            .get(&placement.prefab_id)
            .ok_or_else(|| SceneError::CookedAssetReference {
                asset_id: placement.prefab_id,
                reason: "prefab ID is not present in cooked/index.ron".to_owned(),
            })?;
    if prefab_location.node.is_some() {
        return Err(SceneError::CookedAssetReference {
            asset_id: placement.prefab_id,
            reason: "prefab ID must reference a prefab data file".to_owned(),
        });
    }
    let relative = Path::new(&prefab_location.pack);
    if !safe_cooked_path(relative) {
        return Err(SceneError::CookedAssetReference {
            asset_id: placement.prefab_id,
            reason: "prefab path must stay inside the cooked directory".to_owned(),
        });
    }
    let prefab_path = cooked_pack_path(placement.prefab_id, relative)?;
    let prefab_text = content
        .load_text(&prefab_path)
        .map_err(|error| SceneError::Prefab {
            file: prefab_path.to_string(),
            field: "file".to_owned(),
            reason: error.to_string(),
        })?;
    let prefab = parse_prefab(prefab_text, prefab_path.as_str())?;
    if prefab.asset_id != placement.prefab_id {
        return Err(SceneError::Prefab {
            file: prefab_path.to_string(),
            field: "asset_id".to_owned(),
            reason: "does not match the placement prefab_id".to_owned(),
        });
    }
    registry.validate(prefab_path.as_str(), &prefab, &placement.overrides)?;
    let transform = transform_from_level(placement.transform)?;
    let entity = world.spawn()?;
    let inserted = world.insert(entity, transform).and_then(|()| {
        world.insert(
            entity,
            Visual {
                mesh: MeshKind::Cooked(prefab.model_id),
                look: Look {
                    material: MaterialKind::Wall,
                    tint: None,
                },
            },
        )
    });
    if let Err(error) = inserted {
        world.despawn(entity)?;
        return Err(error.into());
    }
    let collider = if prefab.collision == PrefabCollision::Cooked {
        let definition = CookedColliderDef {
            asset_id: prefab.model_id,
            transform: placement.transform,
            sensor: false,
        };
        match spawn_cooked_collider(world, &definition, content, index, packs, shape_cache) {
            Ok(collider) => {
                world.despawn(collider.id)?;
                Some(SpawnedCollider {
                    id: entity,
                    body: collider.body,
                })
            }
            Err(error) => {
                world.despawn(entity)?;
                return Err(error);
            }
        }
    } else {
        None
    };
    let context = PrefabSpawn {
        entity,
        prefab_id: placement.prefab_id,
        transform: placement.transform,
    };
    if let Err(error) = registry.spawn(
        world,
        prefab_path.as_str(),
        &prefab,
        context,
        &placement.overrides,
    ) {
        world.despawn(entity)?;
        if let Some(collider) = collider {
            world
                .resource_mut::<PhysicsWorld>()?
                .remove_body(collider.body)?;
        }
        return Err(error);
    }
    Ok((entity, collider))
}

pub(super) fn transform_from_level(transform: LevelTransform) -> Result<Transform, SceneError> {
    let [yaw, pitch, roll] = transform.rotation_deg.map(f32::to_radians);
    Transform::new(
        Vec3::from_array(transform.translation),
        Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll),
        Vec3::from_array(transform.scale),
    )
    .map_err(SceneError::from)
}

fn safe_cooked_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
}

fn cooked_pack_path(asset_id: AssetId, relative: &Path) -> Result<AssetPath, SceneError> {
    if !safe_cooked_path(relative) {
        return Err(SceneError::CookedAssetReference {
            asset_id,
            reason: "index pack path must stay inside the cooked directory".to_owned(),
        });
    }
    AssetPath::new(format!("content/cooked/{}", relative.display())).map_err(|error| {
        SceneError::CookedAssetReference {
            asset_id,
            reason: error.to_string(),
        }
    })
}

fn scale_cooked_shape(shape: &CollisionShape, scale: Vec3) -> CollisionShape {
    match shape {
        CollisionShape::Box {
            translation,
            rotation,
            half_extents,
        } => CollisionShape::Box {
            translation: (Vec3::from_array(*translation) * scale).to_array(),
            rotation: *rotation,
            half_extents: (Vec3::from_array(*half_extents) * scale.abs()).to_array(),
        },
        CollisionShape::Hull { points } => CollisionShape::Hull {
            points: points
                .iter()
                .map(|point| (Vec3::from_array(*point) * scale).to_array())
                .collect(),
        },
        CollisionShape::TriMesh { positions, indices } => CollisionShape::TriMesh {
            positions: positions
                .iter()
                .map(|point| (Vec3::from_array(*point) * scale).to_array())
                .collect(),
            indices: indices.clone(),
        },
        CollisionShape::Decomposed { hulls } => CollisionShape::Decomposed {
            hulls: hulls
                .iter()
                .map(|hull| {
                    hull.iter()
                        .map(|point| (Vec3::from_array(*point) * scale).to_array())
                        .collect()
                })
                .collect(),
        },
    }
}
