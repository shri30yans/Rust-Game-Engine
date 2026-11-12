//! Canonical spawned-entry ownership and ordered scene views.
use std::collections::BTreeMap;

use engine_assets::format::AssetId;
use engine_core::StateHasher;
use engine_physics::BodyId;
use engine_world::{NetId, Resource, Transform};

use crate::model::Level;

use super::types::{Markers, SpawnedBody, SpawnedCollider};

/// The canonical runtime representation of one authored record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnedEntry {
    /// Entity spawned for this authored source record.
    pub id: NetId,
    /// Physics body attached to the entity, when the record has collision.
    pub body: Option<BodyId>,
    /// Transform last accepted from authored scene content.
    pub authored: Transform,
}

/// Everything one level spawned: canonical source entries, role lists and markers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spawned {
    pub(super) entries: BTreeMap<AssetId, SpawnedEntry>,
    pub(super) bodies: Vec<SpawnedBody>,
    pub(super) colliders: Vec<SpawnedCollider>,
    pub(super) instances: Vec<NetId>,
    pub(super) instance_bodies: Vec<Option<BodyId>>,
    pub(super) prefabs: Vec<NetId>,
    pub(super) markers: Markers,
}

impl Spawned {
    /// Canonical spawned entities by authored source ID.
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<AssetId, SpawnedEntry> {
        &self.entries
    }

    /// Removes one canonical entry so its caller can safely despawn it.
    pub fn take_entry(&mut self, source_id: AssetId) -> Option<SpawnedEntry> {
        self.entries.remove(&source_id)
    }

    /// Records an entity created through the scene spawn path for one source ID.
    pub fn insert_entry(
        &mut self,
        source_id: AssetId,
        entry: SpawnedEntry,
    ) -> Option<SpawnedEntry> {
        self.entries.insert(source_id, entry)
    }

    /// Updates the authored transform after a successful live move.
    pub fn set_authored(&mut self, source_id: AssetId, authored: Transform) -> bool {
        let Some(entry) = self.entries.get_mut(&source_id) else {
            return false;
        };
        entry.authored = authored;
        true
    }

    /// Rebuilds ordered views from canonical entries in the accepted level's file order.
    ///
    /// This is the sole bridge from stable spawned ownership to compatibility views. An entry
    /// absent after gameplay claimed or destroyed it is intentionally absent from the views.
    pub fn rebuild_positional_views(&mut self, level: &Level) {
        self.bodies.clear();
        self.colliders.clear();
        self.instances.clear();
        self.instance_bodies.clear();
        self.prefabs.clear();

        for shape in level.shapes() {
            let Some(entry) = self.entries.get(&shape.id()) else {
                continue;
            };
            let Some(body) = entry.body else {
                continue;
            };
            self.bodies.push(SpawnedBody {
                id: entry.id,
                body,
                kind: shape.kind(),
                look: shape.look(),
            });
        }
        for definition in level.instances() {
            let Some(entry) = self.entries.get(&definition.asset_id) else {
                continue;
            };
            self.instances.push(entry.id);
            self.instance_bodies.push(entry.body);
        }
        for definition in level.colliders() {
            let Some(entry) = self.entries.get(&definition.asset_id) else {
                continue;
            };
            let Some(body) = entry.body else {
                continue;
            };
            self.colliders.push(SpawnedCollider { id: entry.id, body });
        }
        for placement in level.prefabs() {
            let Some(entry) = self.entries.get(&placement.id) else {
                continue;
            };
            self.prefabs.push(entry.id);
            if let Some(body) = entry.body {
                self.colliders.push(SpawnedCollider { id: entry.id, body });
            }
        }
        self.markers = Markers::of(level);
    }

    /// The spawned bodies, in spawn order.
    #[must_use]
    pub fn bodies(&self) -> &[SpawnedBody] {
        &self.bodies
    }

    /// Cooked collision placements, in file order.
    #[must_use]
    pub fn colliders(&self) -> &[SpawnedCollider] {
        &self.colliders
    }

    /// Cooked mesh instance entities, in file order.
    #[must_use]
    pub fn instances(&self) -> &[NetId] {
        &self.instances
    }

    /// Physics bodies for [`Spawned::instances`] at matching file-order positions.
    #[must_use]
    pub fn instance_bodies(&self) -> &[Option<BodyId>] {
        &self.instance_bodies
    }

    /// Prefab placement entities, in file order.
    #[must_use]
    pub fn prefabs(&self) -> &[NetId] {
        &self.prefabs
    }

    /// The markers, in file order.
    #[must_use]
    pub fn markers(&self) -> &Markers {
        &self.markers
    }
}

/// The spawned set of the level currently in the world. A world resource: a reload reads it to
/// remove the old level, and its hash covers the ids so equal scenes hash equally.
#[derive(Debug, Default)]
pub struct LevelInstance {
    pub(super) spawned: Spawned,
}

impl Resource for LevelInstance {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for (source_id, entry) in &self.spawned.entries {
            hasher.write_u64(source_id.get());
            hasher.write_u32(entry.id.index());
            hasher.write_u32(entry.id.generation());
            if let Some(body) = entry.body {
                hasher.write_u32(body.index());
                hasher.write_u32(body.generation());
            }
        }
        for entry in &self.spawned.bodies {
            hasher.write_u32(entry.id.index());
            hasher.write_u32(entry.id.generation());
            hasher.write_u32(entry.body.index());
            hasher.write_u32(entry.body.generation());
        }
        for entry in &self.spawned.colliders {
            hasher.write_u32(entry.id.index());
            hasher.write_u32(entry.id.generation());
            hasher.write_u32(entry.body.index());
            hasher.write_u32(entry.body.generation());
        }
        for id in &self.spawned.instances {
            hasher.write_u32(id.index());
            hasher.write_u32(id.generation());
        }
        for id in &self.spawned.prefabs {
            hasher.write_u32(id.index());
            hasher.write_u32(id.generation());
        }
    }
}
