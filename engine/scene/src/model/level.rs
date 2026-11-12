//! Raw level input, the validated level, and whole-level consistency checks.
use std::collections::HashSet;

use engine_assets::format::AssetId;
use serde::{Deserialize, Serialize};

use crate::error::SceneError;

use super::validation::{finite, non_empty, positive};
use super::{
    CookedColliderDef, CueDef, InstanceDef, InvalidReason, LayoutDef, LevelRecord, LevelRecordDef,
    MAX_COLLIDERS, MAX_CUES, MAX_INSTANCES, MAX_LAYOUTS, MAX_SHAPES, MAX_VOLUMES, Marker,
    PrefabPlacement, RawShape, SUPPORTED_VERSION, ShapeDef, ValidationError, VolumeDef,
};

/// A level file as serde reads it. Not usable until [`Level::try_from`] accepts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLevel {
    /// Must equal [`SUPPORTED_VERSION`].
    pub version: u32,
    /// Level name.
    pub name: String,
    /// Gravity in metres per second squared.
    pub gravity: [f32; 3],
    /// The shapes, spawned in this order.
    #[serde(default)]
    pub shapes: Vec<RawShape>,
    /// Player (and bot) spawn points, in order.
    #[serde(default)]
    pub spawns: Vec<Marker>,
    /// Waypoints, in order.
    #[serde(default)]
    pub waypoints: Vec<Marker>,
    /// Other named markers, in order.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// Cooked render mesh instances.
    #[serde(default)]
    pub instances: Vec<InstanceDef>,
    /// Cooked collision sets. Greybox colliders remain the colliders of `shapes`.
    #[serde(default)]
    pub colliders: Vec<CookedColliderDef>,
    /// Reusable prefab placements.
    #[serde(default)]
    pub prefabs: Vec<PrefabPlacement>,
    /// Tagged gameplay and presentation volumes.
    #[serde(default)]
    pub volumes: Vec<VolumeDef>,
    /// Rest poses for named arena layouts.
    #[serde(default)]
    pub layouts: Vec<LayoutDef>,
    /// Named Director cues.
    #[serde(default)]
    pub cues: Vec<CueDef>,
}

/// A validated level. Build it with [`Level::try_from`] or parse text with [`Level::parse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Level {
    version: u32,
    name: String,
    gravity: [f32; 3],
    shapes: Vec<ShapeDef>,
    spawns: Vec<Marker>,
    waypoints: Vec<Marker>,
    markers: Vec<Marker>,
    instances: Vec<InstanceDef>,
    colliders: Vec<CookedColliderDef>,
    prefabs: Vec<PrefabPlacement>,
    volumes: Vec<VolumeDef>,
    layouts: Vec<LayoutDef>,
    cues: Vec<CueDef>,
}

impl Level {
    /// Parses and validates level text. `file` names the source in every error.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError::Parse`] when the text is not valid RON for a level, and
    /// [`SceneError::Invalid`] when a value breaks a rule.
    pub fn parse(text: &str, file: &str) -> Result<Self, SceneError> {
        let raw: RawLevel = ron::from_str(text).map_err(|source| SceneError::Parse {
            file: file.to_owned(),
            source,
        })?;
        Self::try_from(raw).map_err(|error| SceneError::Invalid {
            file: file.to_owned(),
            error,
        })
    }

    /// Writes the level as RON text. Parsing the result gives back an equal level.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError::Encode`] when serialisation fails.
    pub fn to_ron(&self) -> Result<String, SceneError> {
        Ok(ron::to_string(self)?)
    }

    /// The format version (always [`SUPPORTED_VERSION`]).
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The level name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gravity in metres per second squared.
    #[must_use]
    pub const fn gravity(&self) -> [f32; 3] {
        self.gravity
    }

    /// The shapes, in spawn order.
    #[must_use]
    pub fn shapes(&self) -> &[ShapeDef] {
        &self.shapes
    }

    /// The spawn points, in file order.
    #[must_use]
    pub fn spawns(&self) -> &[Marker] {
        &self.spawns
    }

    /// The waypoints, in file order.
    #[must_use]
    pub fn waypoints(&self) -> &[Marker] {
        &self.waypoints
    }

    /// The other markers, in file order.
    #[must_use]
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Cooked render mesh instances in file order.
    #[must_use]
    pub fn instances(&self) -> &[InstanceDef] {
        &self.instances
    }

    /// One entity-producing record for each authored object.
    ///
    /// Markers, volumes, layouts and cues describe level metadata and are deliberately absent:
    /// they do not own a spawned entity and therefore never take part in live entity changes.
    pub fn records(&self) -> impl Iterator<Item = LevelRecord<'_>> {
        self.shapes()
            .iter()
            .map(LevelRecord::Shape)
            .chain(self.instances().iter().map(LevelRecord::Instance))
            .chain(self.colliders().iter().map(LevelRecord::Collider))
            .chain(self.prefabs().iter().map(LevelRecord::Prefab))
    }

    /// Owned entity-producing records for change application at an app boundary.
    pub fn record_definitions(&self) -> impl Iterator<Item = LevelRecordDef> + '_ {
        self.records().map(LevelRecordDef::from)
    }

    /// Cooked collision sets in file order.
    #[must_use]
    pub fn colliders(&self) -> &[CookedColliderDef] {
        &self.colliders
    }

    /// Reusable prefab placements in file order.
    #[must_use]
    pub fn prefabs(&self) -> &[PrefabPlacement] {
        &self.prefabs
    }

    /// Tagged volumes in file order.
    #[must_use]
    pub fn volumes(&self) -> &[VolumeDef] {
        &self.volumes
    }

    /// Authored layouts in file order.
    #[must_use]
    pub fn layouts(&self) -> &[LayoutDef] {
        &self.layouts
    }

    /// Named Director cues in file order.
    #[must_use]
    pub fn cues(&self) -> &[CueDef] {
        &self.cues
    }
}

fn check_markers(list: &str, markers: &[Marker]) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for (index, marker) in markers.iter().enumerate() {
        let location = format!("{list} {index} ({:?})", marker.name);
        marker
            .validate()
            .map_err(|reason| ValidationError::new(location.clone(), reason))?;
        if !seen.insert(marker.name.as_str()) {
            return Err(ValidationError::new(
                location,
                InvalidReason::DuplicateName(marker.name.clone()),
            ));
        }
    }
    Ok(())
}

fn check_record_count(
    field: &'static str,
    count: usize,
    limit: usize,
) -> Result<(), ValidationError> {
    if count > limit {
        return Err(ValidationError::new(
            field,
            InvalidReason::TooManyRecords {
                field,
                count,
                limit,
            },
        ));
    }
    Ok(())
}

fn lower_snake_case(value: &str) -> bool {
    let mut previous_underscore = false;
    for (index, character) in value.chars().enumerate() {
        if character == '_' {
            if index == 0 || previous_underscore {
                return false;
            }
            previous_underscore = true;
        } else if character.is_ascii_lowercase() || (index > 0 && character.is_ascii_digit()) {
            previous_underscore = false;
        } else {
            return false;
        }
    }
    !previous_underscore && !value.is_empty()
}

fn check_unique_ids<'a>(
    collection: &str,
    ids: impl IntoIterator<Item = &'a AssetId>,
) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for (index, id) in ids.into_iter().enumerate() {
        if !seen.insert(*id) {
            return Err(ValidationError::new(
                format!("{collection} {index}"),
                InvalidReason::DuplicateName(format!("asset id {}", id.get())),
            ));
        }
    }
    Ok(())
}

fn validate_extended_records(raw: &RawLevel) -> Result<(), ValidationError> {
    check_record_count("instances", raw.instances.len(), MAX_INSTANCES)?;
    check_record_count("colliders", raw.colliders.len(), MAX_COLLIDERS)?;
    check_record_count("prefabs", raw.prefabs.len(), MAX_INSTANCES)?;
    check_record_count("volumes", raw.volumes.len(), MAX_VOLUMES)?;
    check_record_count("layouts", raw.layouts.len(), MAX_LAYOUTS)?;
    check_record_count("cues", raw.cues.len(), MAX_CUES)?;
    let placement_ids = raw
        .shapes
        .iter()
        .map(|shape| &shape.id)
        .chain(raw.instances.iter().map(|instance| &instance.asset_id))
        .chain(raw.colliders.iter().map(|collider| &collider.asset_id))
        .chain(raw.prefabs.iter().map(|prefab| &prefab.id))
        .chain(raw.volumes.iter().map(|volume| &volume.id))
        .collect::<Vec<_>>();
    check_unique_ids("level object", placement_ids)?;
    let mut layout_names = HashSet::new();
    let mut expected_movers = None::<HashSet<AssetId>>;
    let mut expected_switches = None::<HashSet<AssetId>>;
    for (index, layout) in raw.layouts.iter().enumerate() {
        let location = format!("layout {index} ({:?})", layout.name);
        non_empty(&layout.name).map_err(|reason| ValidationError::new(location.clone(), reason))?;
        if !lower_snake_case(&layout.name) {
            return Err(ValidationError::new(
                location.clone(),
                InvalidReason::InvalidNameFormat,
            ));
        }
        if !layout_names.insert(layout.name.as_str()) {
            return Err(ValidationError::new(
                location,
                InvalidReason::DuplicateName(layout.name.clone()),
            ));
        }
        for transform in layout.mover_poses.values() {
            transform
                .validate()
                .map_err(|reason| ValidationError::new(location.clone(), reason))?;
        }
        let movers = layout.mover_poses.keys().copied().collect::<HashSet<_>>();
        let switches = layout.switch_states.keys().copied().collect::<HashSet<_>>();
        if expected_movers
            .as_ref()
            .is_some_and(|known| known != &movers)
            || expected_switches
                .as_ref()
                .is_some_and(|known| known != &switches)
        {
            return Err(ValidationError::new(
                location,
                InvalidReason::LayoutObjectsMismatch,
            ));
        }
        expected_movers.get_or_insert(movers);
        expected_switches.get_or_insert(switches);
    }
    let mut cue_names = HashSet::new();
    for (index, cue) in raw.cues.iter().enumerate() {
        let location = format!("cue {index} ({:?})", cue.name);
        non_empty(&cue.name).map_err(|reason| ValidationError::new(location.clone(), reason))?;
        if !lower_snake_case(&cue.name) {
            return Err(ValidationError::new(
                location.clone(),
                InvalidReason::InvalidNameFormat,
            ));
        }
        if !cue_names.insert(cue.name.as_str()) {
            return Err(ValidationError::new(
                location,
                InvalidReason::DuplicateName(cue.name.clone()),
            ));
        }
        if !layout_names.contains(cue.layout.as_str()) {
            return Err(ValidationError::new(
                location,
                InvalidReason::UnknownLayout(cue.layout.clone()),
            ));
        }
        positive("duration_s", &[cue.duration_s])
            .map_err(|reason| ValidationError::new(format!("cue {index} duration_s"), reason))?;
    }
    for (index, instance) in raw.instances.iter().enumerate() {
        instance
            .transform
            .validate()
            .map_err(|reason| ValidationError::new(format!("instance {index}"), reason))?;
    }
    for (index, collider) in raw.colliders.iter().enumerate() {
        collider
            .transform
            .validate()
            .map_err(|reason| ValidationError::new(format!("collider {index}"), reason))?;
    }
    for (index, prefab) in raw.prefabs.iter().enumerate() {
        prefab
            .transform
            .validate()
            .map_err(|reason| ValidationError::new(format!("prefab {index}"), reason))?;
    }
    let mut volume_names = HashSet::new();
    for (index, volume) in raw.volumes.iter().enumerate() {
        let location = format!("volume {index} ({:?})", volume.name);
        non_empty(&volume.name).map_err(|reason| ValidationError::new(location.clone(), reason))?;
        if !volume_names.insert(volume.name.as_str()) {
            return Err(ValidationError::new(
                location,
                InvalidReason::DuplicateName(volume.name.clone()),
            ));
        }
        finite("center", &volume.center)
            .map_err(|reason| ValidationError::new(location.clone(), reason))?;
        positive("half_extents", &volume.half_extents)
            .map_err(|reason| ValidationError::new(location, reason))?;
    }
    Ok(())
}

impl TryFrom<RawLevel> for Level {
    type Error = ValidationError;

    fn try_from(raw: RawLevel) -> Result<Self, Self::Error> {
        if raw.version != SUPPORTED_VERSION {
            return Err(ValidationError::new(
                "version",
                InvalidReason::Version {
                    found: raw.version,
                    expected: SUPPORTED_VERSION,
                },
            ));
        }
        non_empty(&raw.name).map_err(|reason| ValidationError::new("level name", reason))?;
        finite("gravity", &raw.gravity)
            .map_err(|reason| ValidationError::new("gravity", reason))?;
        if raw.shapes.len() > MAX_SHAPES {
            return Err(ValidationError::new(
                "shapes",
                InvalidReason::TooManyShapes {
                    count: raw.shapes.len(),
                    limit: MAX_SHAPES,
                },
            ));
        }
        validate_extended_records(&raw)?;
        let mut names = HashSet::new();
        let mut shapes = Vec::with_capacity(raw.shapes.len());
        for (index, raw_shape) in raw.shapes.into_iter().enumerate() {
            let label = match &raw_shape.name {
                Some(text) => format!("shape {index} ({text:?})"),
                None => format!("shape {index}"),
            };
            let shape_name = raw_shape.name.clone();
            let shape = ShapeDef::try_from(raw_shape)
                .map_err(|reason| ValidationError::new(label.clone(), reason))?;
            if let Some(text) = shape_name
                && !names.insert(text.clone())
            {
                return Err(ValidationError::new(
                    label,
                    InvalidReason::DuplicateName(text),
                ));
            }
            shapes.push(shape);
        }
        check_markers("spawn", &raw.spawns)?;
        check_markers("waypoint", &raw.waypoints)?;
        check_markers("marker", &raw.markers)?;
        Ok(Self {
            version: raw.version,
            name: raw.name,
            gravity: raw.gravity,
            shapes,
            spawns: raw.spawns,
            waypoints: raw.waypoints,
            markers: raw.markers,
            instances: raw.instances,
            colliders: raw.colliders,
            prefabs: raw.prefabs,
            volumes: raw.volumes,
            layouts: raw.layouts,
            cues: raw.cues,
        })
    }
}
