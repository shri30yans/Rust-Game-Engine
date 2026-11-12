//! Game-extensible prefab records and their typed component-block registry.
//!
//! The engine validates block names and serde shapes but never defines game blocks. Every error
//! carries the prefab filename and component field so a data author can fix the source record.

use std::any::Any;
use std::collections::BTreeMap;

use engine_assets::format::AssetId;
use engine_world::{NetId, World};
use ron::Value;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{LevelTransform, SceneError};

/// Collision behavior selected by a prefab definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum PrefabCollision {
    /// The prefab has no physics shape.
    None,
    /// Resolve the cooked collision set beside the model asset.
    Cooked,
}

/// A prefab asset stored as `content/prefabs/<name>.prefab.ron`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabDef {
    /// Stable ID of this prefab definition file.
    pub asset_id: AssetId,
    /// Stable ID of its model object.
    pub model_id: AssetId,
    /// Collision behavior for placements.
    pub collision: PrefabCollision,
    /// Engine-neutral surface name.
    pub surface: String,
    /// Stable IDs of sounds used by this prefab.
    #[serde(default)]
    pub sounds: Vec<AssetId>,
    /// Stable IDs of effects used by this prefab.
    #[serde(default)]
    pub effects: Vec<AssetId>,
    /// Typed component name to serde record.
    #[serde(default)]
    pub components: BTreeMap<String, Value>,
}

/// Parses and validates a prefab definition from the mounted content pack.
pub fn parse_prefab(text: &str, file: &str) -> Result<PrefabDef, SceneError> {
    let prefab: PrefabDef = ron::from_str(text).map_err(|source| SceneError::Parse {
        file: file.to_owned(),
        source,
    })?;
    if prefab.surface.trim().is_empty() {
        return Err(prefab_error(file, "surface", "must not be empty"));
    }
    Ok(prefab)
}

/// Context supplied by the engine when a game block is spawned.
#[derive(Debug, Clone, Copy)]
pub struct PrefabSpawn {
    /// Placement entity.
    pub entity: NetId,
    /// Stable ID of the prefab definition.
    pub prefab_id: AssetId,
    /// Authored world-space transform.
    pub transform: LevelTransform,
}

type DecodeBlock = fn(Value) -> Result<Box<dyn Any>, String>;
type SpawnBlock = Box<dyn Fn(&mut World, PrefabSpawn, Box<dyn Any>) -> Result<(), SceneError>>;

struct BlockRegistration {
    decode: DecodeBlock,
    spawn: SpawnBlock,
}

/// Registry of component blocks understood by one game.
#[derive(Default)]
pub struct PrefabRegistry {
    blocks: BTreeMap<String, BlockRegistration>,
}

struct PreparedBlock<'a> {
    registration: &'a BlockRegistration,
    value: Box<dyn Any>,
}

impl PrefabRegistry {
    /// Registers a game-owned serde record and the function that adds it to a placement entity.
    ///
    /// # Errors
    /// Returns an error if the block name is empty or already registered.
    pub fn register<T>(
        &mut self,
        name: impl Into<String>,
        spawn: fn(&mut World, PrefabSpawn, T) -> Result<(), SceneError>,
    ) -> Result<(), SceneError>
    where
        T: DeserializeOwned + 'static,
    {
        let name = name.into();
        if name.trim().is_empty() || self.blocks.contains_key(&name) {
            return Err(prefab_error(
                "<registry>",
                &name,
                "block name is empty or duplicated",
            ));
        }
        let field_name = name.clone();
        self.blocks.insert(
            name,
            BlockRegistration {
                decode: decode_block::<T>,
                spawn: Box::new(move |world, placement, block| {
                    let block = block.downcast::<T>().map_err(|_| {
                        prefab_error("<registry>", &field_name, "registered block type changed")
                    })?;
                    spawn(world, placement, *block)
                }),
            },
        );
        Ok(())
    }

    /// Checks every component block and applies placement field overrides before spawning.
    ///
    /// # Errors
    /// Returns an error naming the prefab file and component field for unknown blocks or invalid
    /// serde values.
    pub fn spawn(
        &self,
        world: &mut World,
        file: &str,
        prefab: &PrefabDef,
        placement: PrefabSpawn,
        overrides: &BTreeMap<String, BTreeMap<String, Value>>,
    ) -> Result<(), SceneError> {
        let prepared = self.prepare(file, prefab, overrides)?;
        for block in prepared {
            (block.registration.spawn)(world, placement, block.value)
                .map_err(|error| prefab_error(file, "spawn", &error.to_string()))?;
        }
        Ok(())
    }

    /// Validates all component types and overrides without changing the world.
    ///
    /// # Errors
    /// Returns an error naming the prefab file and component field for unknown blocks or invalid
    /// serde values.
    pub fn validate(
        &self,
        file: &str,
        prefab: &PrefabDef,
        overrides: &BTreeMap<String, BTreeMap<String, Value>>,
    ) -> Result<(), SceneError> {
        self.prepare(file, prefab, overrides).map(|_| ())
    }

    fn prepare(
        &self,
        file: &str,
        prefab: &PrefabDef,
        overrides: &BTreeMap<String, BTreeMap<String, Value>>,
    ) -> Result<Vec<PreparedBlock<'_>>, SceneError> {
        for (name, fields) in overrides {
            if !prefab.components.contains_key(name) {
                return Err(prefab_error(
                    file,
                    name,
                    "override names a missing component block",
                ));
            }
            if fields.is_empty() {
                return Err(prefab_error(
                    file,
                    name,
                    "override must set at least one field",
                ));
            }
        }
        let mut prepared = Vec::with_capacity(prefab.components.len());
        for (name, value) in &prefab.components {
            let registration = self
                .blocks
                .get(name)
                .ok_or_else(|| prefab_error(file, name, "component block is not registered"))?;
            let merged = merge_override(file, name, value.clone(), overrides.get(name))?;
            let block = (registration.decode)(merged)
                .map_err(|reason| prefab_error(file, name, &reason))?;
            prepared.push(PreparedBlock {
                registration,
                value: block,
            });
        }
        Ok(prepared)
    }
}

fn decode_block<T: DeserializeOwned + 'static>(value: Value) -> Result<Box<dyn Any>, String> {
    value
        .into_rust::<T>()
        .map(|block| Box::new(block) as Box<dyn Any>)
        .map_err(|error| error.to_string())
}

fn merge_override(
    file: &str,
    component: &str,
    mut value: Value,
    overrides: Option<&BTreeMap<String, Value>>,
) -> Result<Value, SceneError> {
    let Some(overrides) = overrides else {
        return Ok(value);
    };
    let Value::Map(fields) = &mut value else {
        return Err(prefab_error(
            file,
            component,
            "component record must be a named map",
        ));
    };
    for (name, override_value) in overrides {
        fields.insert(Value::String(name.clone()), override_value.clone());
    }
    Ok(value)
}

fn prefab_error(file: &str, field: &str, reason: &str) -> SceneError {
    SceneError::Prefab {
        file: file.to_owned(),
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use engine_world::Replication;

    #[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Door {
        open_s: f32,
    }

    fn spawn_door(
        world: &mut World,
        placement: PrefabSpawn,
        block: Door,
    ) -> Result<(), SceneError> {
        world.insert(placement.entity, block)?;
        Ok(())
    }

    fn registry() -> PrefabRegistry {
        let mut registry = PrefabRegistry::default();
        registry
            .register("door", spawn_door)
            .expect("register block");
        registry
    }

    fn prefab(components: BTreeMap<String, Value>) -> PrefabDef {
        PrefabDef {
            asset_id: AssetId::new(1).expect("asset id"),
            model_id: AssetId::new(2).expect("model id"),
            collision: PrefabCollision::Cooked,
            surface: "steel".to_owned(),
            sounds: Vec::new(),
            effects: Vec::new(),
            components,
        }
    }

    fn door_component(open_s: Value) -> Value {
        [(Value::from("open_s"), open_s)].into_iter().collect()
    }

    fn placement(entity: NetId) -> PrefabSpawn {
        PrefabSpawn {
            entity,
            prefab_id: AssetId::new(1).expect("prefab id"),
            transform: LevelTransform {
                translation: [0.0; 3],
                rotation_deg: [0.0; 3],
                scale: [1.0; 3],
            },
        }
    }

    #[test]
    fn unknown_component_block_names_the_prefab_and_field() {
        let prefab = prefab(BTreeMap::from([(
            "mystery".to_owned(),
            door_component(Value::from(0.6_f32)),
        )]));
        let mut world = World::new(1).expect("world");
        let entity = world.spawn().expect("entity");
        let error = registry()
            .spawn(
                &mut world,
                "door.prefab.ron",
                &prefab,
                placement(entity),
                &BTreeMap::new(),
            )
            .expect_err("unknown block");
        let message = error.to_string();
        assert!(message.contains("door.prefab.ron"), "{message}");
        assert!(message.contains("mystery"), "{message}");
    }

    #[test]
    fn malformed_component_record_names_the_prefab_and_field() {
        let prefab = prefab(BTreeMap::from([(
            "door".to_owned(),
            door_component(Value::from("slow")),
        )]));
        let mut world = World::new(1).expect("world");
        let entity = world.spawn().expect("entity");
        let error = registry()
            .spawn(
                &mut world,
                "door.prefab.ron",
                &prefab,
                placement(entity),
                &BTreeMap::new(),
            )
            .expect_err("bad type");
        let message = error.to_string();
        assert!(message.contains("door.prefab.ron"), "{message}");
        assert!(message.contains("door"), "{message}");
    }

    #[test]
    fn overrides_are_merged_and_spawned_through_the_registered_type() {
        let prefab = prefab(BTreeMap::from([(
            "door".to_owned(),
            door_component(Value::from(0.6_f32)),
        )]));
        let mut world = World::new(1).expect("world");
        world
            .register::<Door>(Replication::ServerOnly)
            .expect("register component");
        let entity = world.spawn().expect("entity");
        let overrides = BTreeMap::from([(
            "door".to_owned(),
            BTreeMap::from([("open_s".to_owned(), Value::from(0.9_f32))]),
        )]);
        registry()
            .spawn(
                &mut world,
                "door.prefab.ron",
                &prefab,
                placement(entity),
                &overrides,
            )
            .expect("spawn override");
        assert_eq!(world.get::<Door>(entity).expect("door block").open_s, 0.9);
    }

    #[test]
    fn override_for_a_missing_component_is_refused() {
        let prefab = prefab(BTreeMap::new());
        let mut world = World::new(1).expect("world");
        let entity = world.spawn().expect("entity");
        let overrides = BTreeMap::from([(
            "door".to_owned(),
            BTreeMap::from([("open_s".to_owned(), Value::from(0.9_f32))]),
        )]);
        let error = registry()
            .spawn(
                &mut world,
                "crate.prefab.ron",
                &prefab,
                placement(entity),
                &overrides,
            )
            .expect_err("orphan override");
        assert!(error.to_string().contains("crate.prefab.ron"));
        assert!(error.to_string().contains("door"));
    }

    #[test]
    fn all_blocks_are_validated_before_any_block_spawns() {
        let prefab = prefab(BTreeMap::from([
            ("door".to_owned(), door_component(Value::from(0.6_f32))),
            ("unknown".to_owned(), door_component(Value::from(0.5_f32))),
        ]));
        let mut world = World::new(1).expect("world");
        world
            .register::<Door>(Replication::ServerOnly)
            .expect("register component");
        let entity = world.spawn().expect("entity");
        let error = registry()
            .spawn(
                &mut world,
                "mixed.prefab.ron",
                &prefab,
                placement(entity),
                &BTreeMap::new(),
            )
            .expect_err("unknown block");
        assert!(error.to_string().contains("unknown"));
        assert!(world.get::<Door>(entity).is_err());
    }
}
