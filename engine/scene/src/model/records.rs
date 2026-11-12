//! Authored records that make up a level, including entities and metadata.
use std::collections::BTreeMap;

use engine_assets::format::AssetId;
use serde::{Deserialize, Serialize};

use super::InvalidReason;
use super::shapes::ShapeDef;
use super::validation::{finite, non_empty, positive};

/// A named point in the level: a spawn, a waypoint or a free marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Name, unique within its list.
    pub name: String,
    /// Position in metres.
    pub translation: [f32; 3],
    /// Facing about Y, in degrees.
    #[serde(default)]
    pub yaw_deg: f32,
}

impl Marker {
    pub(super) fn validate(&self) -> Result<(), InvalidReason> {
        non_empty(&self.name)?;
        finite("translation", &self.translation)?;
        finite("yaw_deg", &[self.yaw_deg])
    }
}

/// A validated level-space transform for cooked instances and movers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelTransform {
    /// Translation in metres.
    pub translation: [f32; 3],
    /// Rotation in yaw, pitch, roll degrees.
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    /// Positive scale on each local axis.
    #[serde(default = "unit_scale")]
    pub scale: [f32; 3],
}

const fn unit_scale() -> [f32; 3] {
    [1.0; 3]
}

impl LevelTransform {
    pub(super) fn validate(&self) -> Result<(), InvalidReason> {
        finite("translation", &self.translation)?;
        finite("rotation_deg", &self.rotation_deg)?;
        positive("scale", &self.scale)
    }
}

/// A cooked mesh node placed in the level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceDef {
    /// Stable ID of the source object node.
    pub asset_id: AssetId,
    /// World-space placement.
    pub transform: LevelTransform,
    /// Optional material slot override in the cooked pack.
    #[serde(default)]
    pub material_override: Option<u32>,
    /// Cooked collision attached to this same authored object.
    #[serde(default)]
    pub collision: Option<InstanceCollision>,
}

/// Cooked collision attached to its containing [`InstanceDef`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceCollision {
    /// Whether the collision is query-only.
    #[serde(default)]
    pub sensor: bool,
}

/// A cooked collision set placed independently of the render mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookedColliderDef {
    /// Stable ID of the source object node with a cooked collision set.
    pub asset_id: AssetId,
    /// World-space placement.
    pub transform: LevelTransform,
    /// Whether the set is query-only.
    #[serde(default)]
    pub sensor: bool,
}

/// A borrowed entity-producing authored record.
///
/// Non-spawned metadata records are intentionally excluded. Their live handling belongs to their
/// owning gameplay or presentation system rather than the entity spawn pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LevelRecord<'a> {
    /// A primitive shape.
    Shape(&'a ShapeDef),
    /// A cooked visual, optionally with collision attached to the same entity.
    Instance(&'a InstanceDef),
    /// A collision-only cooked object.
    Collider(&'a CookedColliderDef),
    /// A prefab placement.
    Prefab(&'a PrefabPlacement),
}

impl LevelRecord<'_> {
    /// Stable authored source ID.
    #[must_use]
    pub const fn id(self) -> AssetId {
        match self {
            Self::Shape(record) => record.id(),
            Self::Instance(record) => record.asset_id,
            Self::Collider(record) => record.asset_id,
            Self::Prefab(record) => record.id,
        }
    }

    /// The record's authored placement.
    #[must_use]
    pub fn transform(self) -> LevelTransform {
        match self {
            Self::Shape(record) => LevelTransform {
                translation: record.translation(),
                rotation_deg: record.rotation_deg(),
                scale: record.kind().full_size(),
            },
            Self::Instance(record) => record.transform,
            Self::Collider(record) => record.transform,
            Self::Prefab(record) => record.transform,
        }
    }
}

/// An owned entity-producing authored record used at scene boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelRecordDef {
    /// A primitive shape.
    Shape(ShapeDef),
    /// A cooked visual, optionally with collision attached to the same entity.
    Instance(InstanceDef),
    /// A collision-only cooked object.
    Collider(CookedColliderDef),
    /// A prefab placement.
    Prefab(PrefabPlacement),
}

impl LevelRecordDef {
    /// Stable authored source ID.
    #[must_use]
    pub const fn id(&self) -> AssetId {
        match self {
            Self::Shape(record) => record.id(),
            Self::Instance(record) => record.asset_id,
            Self::Collider(record) => record.asset_id,
            Self::Prefab(record) => record.id,
        }
    }

    /// The record's authored placement.
    #[must_use]
    pub fn transform(&self) -> LevelTransform {
        match self {
            Self::Shape(record) => LevelTransform {
                translation: record.translation(),
                rotation_deg: record.rotation_deg(),
                scale: record.kind().full_size(),
            },
            Self::Instance(record) => record.transform,
            Self::Collider(record) => record.transform,
            Self::Prefab(record) => record.transform,
        }
    }
}

impl From<LevelRecord<'_>> for LevelRecordDef {
    fn from(record: LevelRecord<'_>) -> Self {
        match record {
            LevelRecord::Shape(record) => Self::Shape(record.clone()),
            LevelRecord::Instance(record) => Self::Instance(record.clone()),
            LevelRecord::Collider(record) => Self::Collider(record.clone()),
            LevelRecord::Prefab(record) => Self::Prefab(record.clone()),
        }
    }
}

/// A reusable prefab placement with per-instance component field overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabPlacement {
    /// Stable ID for this placement, unique in the level.
    pub id: AssetId,
    /// Stable ID of the prefab data asset.
    pub prefab_id: AssetId,
    /// World-space placement.
    pub transform: LevelTransform,
    /// Component name to field name to untyped RON value. The registry validates these values.
    #[serde(default)]
    pub overrides: BTreeMap<String, BTreeMap<String, ron::Value>>,
}

/// Kind of a tagged level volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeKind {
    /// A mode or gameplay region.
    Zone,
    /// A physics trigger region.
    Trigger,
    /// A light or reflection probe region.
    Probe,
    /// An ambient sound region.
    Sound,
    /// An objective location or capture region.
    Objective,
}

/// A tagged axis-aligned box volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeDef {
    /// Stable ID of this volume.
    pub id: AssetId,
    /// Unique designer-facing name.
    pub name: String,
    /// The gameplay or presentation purpose.
    pub kind: VolumeKind,
    /// Box centre in metres.
    pub center: [f32; 3],
    /// Positive half extents in metres.
    pub half_extents: [f32; 3],
    /// Engine-neutral tags from the source object.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

/// Rest pose and switch state for one authored arena layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutDef {
    /// Unique layout name.
    pub name: String,
    /// Rest transform for every mover used by this layout.
    #[serde(default)]
    pub mover_poses: BTreeMap<AssetId, LevelTransform>,
    /// State of every switchable object in this layout.
    #[serde(default)]
    pub switch_states: BTreeMap<AssetId, bool>,
}

/// An Arena Director cue and the stable object IDs that it controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueDef {
    /// Unique Director event name.
    pub name: String,
    /// Layout selected by this cue.
    pub layout: String,
    /// Time in seconds over which movers and switchable things change state.
    pub duration_s: f32,
}
