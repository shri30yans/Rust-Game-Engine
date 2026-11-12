//! Public spawn results, marker views, and game-supplied scene content.
use engine_assets::ContentPack;
use engine_physics::{BodyId, CollisionLayers, SurfaceId};
use engine_world::NetId;
use std::rc::Rc;

use crate::model::{Level, Look, Marker, ShapeDef, ShapeKind};
use crate::prefab::PrefabRegistry;

/// What one shape spawned: its entity, its physics body and what it looks and is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnedBody {
    /// The entity, carrying `Transform` and `Visual`.
    pub id: NetId,
    /// The physics body (its collider is attached to it).
    pub body: BodyId,
    /// The shape's kind, kept for game-side roles.
    pub kind: ShapeKind,
    /// The shape's look, kept for game-side roles.
    pub look: Look,
}

/// A cooked collision placement spawned without a visual mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnedCollider {
    /// The entity carrying the placement transform.
    pub id: NetId,
    /// The physics body carrying the cooked collision set.
    pub body: BodyId,
}

/// Game-owned collision metadata assigned while a level shape is spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColliderMetadata {
    /// Collision membership and response masks.
    pub layers: CollisionLayers,
    /// Game-owned physical material.
    pub surface: SurfaceId,
    /// Whether the shape is a query-only trigger.
    pub sensor: bool,
}
impl Default for ColliderMetadata {
    fn default() -> Self {
        Self {
            layers: CollisionLayers::ALL,
            surface: SurfaceId::DEFAULT,
            sensor: false,
        }
    }
}

/// Which list a marker belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// A spawn point.
    Spawn,
    /// A waypoint.
    Waypoint,
    /// Any other named marker.
    Other,
}

/// The markers of one level, in file order. Lookup is a linear scan of one short list, so the
/// result is deterministic and no hash map is needed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Markers {
    spawns: Vec<Marker>,
    waypoints: Vec<Marker>,
    others: Vec<Marker>,
}

impl Markers {
    pub(super) fn of(level: &Level) -> Self {
        Self {
            spawns: level.spawns().to_vec(),
            waypoints: level.waypoints().to_vec(),
            others: level.markers().to_vec(),
        }
    }

    /// Every marker of one kind, in file order.
    #[must_use]
    pub fn list(&self, kind: MarkerKind) -> &[Marker] {
        match kind {
            MarkerKind::Spawn => &self.spawns,
            MarkerKind::Waypoint => &self.waypoints,
            MarkerKind::Other => &self.others,
        }
    }

    /// The marker of `kind` called `name`, when the level has one.
    #[must_use]
    pub fn find(&self, kind: MarkerKind, name: &str) -> Option<&Marker> {
        self.list(kind).iter().find(|marker| marker.name == name)
    }
}

/// Game-supplied data and policies used by every cooked scene spawn.
///
/// The content root and prefab registry are owned by the game but consumed only through this
/// engine-scene value. The metadata function maps primitive shapes and cooked surface names to
/// game collision policy.
pub struct SceneContent {
    pub(super) pack: ContentPack,
    pub(super) prefabs: Rc<PrefabRegistry>,
    pub(super) metadata: fn(Option<&ShapeDef>, Option<&str>) -> ColliderMetadata,
}

impl SceneContent {
    /// Creates scene content from the mounted game pack.
    #[must_use]
    pub fn new(
        pack: ContentPack,
        prefabs: PrefabRegistry,
        metadata: fn(Option<&ShapeDef>, Option<&str>) -> ColliderMetadata,
    ) -> Self {
        Self {
            pack,
            prefabs: Rc::new(prefabs),
            metadata,
        }
    }

    /// Empty content for games that only use primitive scene records.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(ContentPack::default(), PrefabRegistry::default(), |_, _| {
            ColliderMetadata::default()
        })
    }

    /// The mounted, validated game content pack.
    #[must_use]
    pub fn pack(&self) -> &ContentPack {
        &self.pack
    }

    /// Reuses this game's prefab and collider policies with the store's current pack revision.
    #[must_use]
    pub fn with_pack(&self, pack: ContentPack) -> Self {
        Self {
            pack,
            prefabs: Rc::clone(&self.prefabs),
            metadata: self.metadata,
        }
    }

    /// Collision policy for a primitive shape or cooked surface.
    #[must_use]
    pub fn collider_metadata(
        &self,
        shape: Option<&ShapeDef>,
        surface: Option<&str>,
    ) -> ColliderMetadata {
        (self.metadata)(shape, surface)
    }

    /// The registered prefab component blocks.
    #[must_use]
    pub fn prefabs(&self) -> &PrefabRegistry {
        &self.prefabs
    }
}
