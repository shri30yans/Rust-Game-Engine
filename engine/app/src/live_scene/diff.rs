//! Stable-ID comparison of validated levels.
use engine_assets::format::AssetId;
use engine_scene::Level;

/// Stable-ID changes between two validated versions of one level.
///
/// Markers, volumes, layouts and cues are data-only records. They own no spawned entity and are
/// intentionally excluded; their owning gameplay or presentation systems consume accepted data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LevelDiff {
    /// Existing entries whose authored placement alone changed.
    pub moved: Vec<engine_scene::LevelRecordDef>,
    /// Existing entries whose non-transform authored data changed.
    pub replaced: Vec<engine_scene::LevelRecordDef>,
    /// Newly authored entries.
    pub added: Vec<engine_scene::LevelRecordDef>,
    /// Source IDs removed from the new level.
    pub removed: Vec<AssetId>,
}

/// Compares every entity-producing record by persistent authored ID.
#[must_use]
pub fn diff_levels(previous: &Level, next: &Level) -> LevelDiff {
    use std::collections::BTreeMap;

    let before = previous
        .record_definitions()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let after = next
        .record_definitions()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let mut diff = LevelDiff::default();
    for (id, current) in &after {
        match before.get(id) {
            None => diff.added.push(current.clone()),
            Some(old)
                if old.transform() != current.transform()
                    && same_record_except_transform(old, current) =>
            {
                diff.moved.push(current.clone());
            }
            Some(old) if old != current => diff.replaced.push(current.clone()),
            Some(_) => {}
        }
    }
    for id in before.keys() {
        if !after.contains_key(id) {
            diff.removed.push(*id);
        }
    }
    diff
}

fn same_record_except_transform(
    previous: &engine_scene::LevelRecordDef,
    next: &engine_scene::LevelRecordDef,
) -> bool {
    match (previous, next) {
        (
            engine_scene::LevelRecordDef::Shape(old),
            engine_scene::LevelRecordDef::Shape(current),
        ) => {
            old.id() == current.id()
                && old.name() == current.name()
                && old.kind() == current.kind()
                && old.body() == current.body()
                && old.look() == current.look()
        }
        (
            engine_scene::LevelRecordDef::Instance(old),
            engine_scene::LevelRecordDef::Instance(current),
        ) => {
            old.asset_id == current.asset_id
                && old.material_override == current.material_override
                && old.collision == current.collision
        }
        (
            engine_scene::LevelRecordDef::Collider(old),
            engine_scene::LevelRecordDef::Collider(current),
        ) => old.asset_id == current.asset_id && old.sensor == current.sensor,
        (
            engine_scene::LevelRecordDef::Prefab(old),
            engine_scene::LevelRecordDef::Prefab(current),
        ) => {
            old.id == current.id
                && old.prefab_id == current.prefab_id
                && old.overrides == current.overrides
        }
        _ => false,
    }
}
