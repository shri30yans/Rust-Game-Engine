//! Cooked visual mesh instance construction.
use engine_assets::format::AssetIndex;
use engine_world::{NetId, Transform, World};
use glam::{EulerRot, Quat, Vec3};

use crate::error::SceneError;
use crate::model::{InstanceDef, LevelTransform, Look, MaterialKind};
use crate::visual::{MeshKind, Visual};

pub(super) fn spawn_cooked_instance(
    world: &mut World,
    definition: &InstanceDef,
    transform_definition: LevelTransform,
    index: &AssetIndex,
) -> Result<NetId, SceneError> {
    let location =
        index
            .get(&definition.asset_id)
            .ok_or_else(|| SceneError::CookedAssetReference {
                asset_id: definition.asset_id,
                reason: "asset ID is not present in cooked/index.ron".to_owned(),
            })?;
    if location.node.is_none() {
        return Err(SceneError::CookedAssetReference {
            asset_id: definition.asset_id,
            reason: "asset ID names a source file, not an object node".to_owned(),
        });
    }
    let [yaw, pitch, roll] = transform_definition.rotation_deg.map(f32::to_radians);
    let transform = Transform::new(
        Vec3::from_array(transform_definition.translation),
        Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll),
        Vec3::from_array(transform_definition.scale),
    )?;
    let id = world.spawn()?;
    let result = world.insert(id, transform).and_then(|()| {
        world.insert(
            id,
            Visual {
                mesh: MeshKind::Cooked(definition.asset_id),
                look: Look {
                    material: MaterialKind::Wall,
                    tint: None,
                },
            },
        )
    });
    if let Err(error) = result {
        world.despawn(id)?;
        return Err(error.into());
    }
    Ok(id)
}
