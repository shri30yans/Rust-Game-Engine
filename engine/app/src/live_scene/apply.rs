//! Atomic application of stable-ID record changes to a running world.
use engine_assets::format::AssetId;
use engine_scene::{Level, SceneError};
use engine_world::{World, WorldError};
use platform::log_error;

use super::LevelDiff;

/// Applies a validated live-level diff at the app boundary.
///
/// An entry changed by gameplay is never overwritten: an absent entity or a transform different
/// from its last accepted authored value is logged and skipped. The canonical spawned map changes
/// only after each successful scene operation.
pub fn apply_level_diff(
    world: &mut World,
    diff: &LevelDiff,
    level: &Level,
    content: &engine_scene::SceneContent,
) -> Result<(), SceneError> {
    for record in &diff.moved {
        let id = record.id();
        let Some(entry) = pristine_entry(world, id)? else {
            continue;
        };
        let target = engine_scene::authored_transform(record)?;
        engine_scene::move_entry(world, entry, target)?;
        let _ = engine_scene::installed_mut(world)?.set_authored(id, target);
    }
    for id in &diff.removed {
        let Some(entry) = pristine_entry(world, *id)? else {
            continue;
        };
        engine_scene::despawn_entry(world, entry)?;
        let _ = engine_scene::installed_mut(world)?.take_entry(*id);
    }
    for record in &diff.replaced {
        replace_record(world, record, content)?;
    }
    for record in &diff.added {
        if engine_scene::installed(world)?
            .entries()
            .contains_key(&record.id())
        {
            return Err(SceneError::World(WorldError::Invariant(
                "added source ID already exists",
            )));
        }
        let entry = engine_scene::spawn_record_from_scene_content(world, record, content)?;
        let _ = engine_scene::installed_mut(world)?.insert_entry(record.id(), entry);
    }
    if !diff.moved.is_empty()
        || !diff.removed.is_empty()
        || !diff.replaced.is_empty()
        || !diff.added.is_empty()
    {
        engine_scene::installed_mut(world)?.rebuild_positional_views(level);
    }
    Ok(())
}

fn replace_record(
    world: &mut World,
    record: &engine_scene::LevelRecordDef,
    content: &engine_scene::SceneContent,
) -> Result<(), SceneError> {
    let id = record.id();
    let Some(previous) = pristine_entry(world, id)? else {
        return Ok(());
    };
    // Build before removing. Presentation extracts only after this app boundary, so it observes
    // either the old accepted primitive or the new one, never a missing intermediate primitive.
    let replacement = engine_scene::spawn_record_from_scene_content(world, record, content)?;
    if let Err(error) = engine_scene::despawn_entry(world, previous) {
        engine_scene::despawn_entry(world, replacement)?;
        return Err(error);
    }
    let _ = engine_scene::installed_mut(world)?.insert_entry(id, replacement);
    Ok(())
}

fn pristine_entry(
    world: &mut World,
    source_id: AssetId,
) -> Result<Option<engine_scene::SpawnedEntry>, SceneError> {
    let Some(entry) = engine_scene::installed(world)?
        .entries()
        .get(&source_id)
        .copied()
    else {
        log_error(&format!(
            "live scene skip {source_id:?}: source entry is absent"
        ));
        return Ok(None);
    };
    if !world.is_alive(entry.id) {
        let _ = engine_scene::installed_mut(world)?.take_entry(source_id);
        log_error(&format!(
            "live scene skip {source_id:?}: entity was destroyed by gameplay"
        ));
        return Ok(None);
    }
    let current = *world.get::<engine_world::Transform>(entry.id)?;
    if current != entry.authored {
        log_error(&format!(
            "live scene skip {source_id:?}: transform changed by gameplay"
        ));
        return Ok(None);
    }
    Ok(Some(entry))
}
