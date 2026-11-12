//! Validated level data, separated into geometry, records, validation, and level assembly.
//!
//! The public schema remains re-exported here so callers use the same `model::Type` paths.

mod level;
mod records;
mod shapes;
mod validation;

#[cfg(test)]
mod tests;

pub use level::{Level, RawLevel};
pub use records::{
    CookedColliderDef, CueDef, InstanceCollision, InstanceDef, LayoutDef, LevelRecord,
    LevelRecordDef, LevelTransform, Marker, PrefabPlacement, VolumeDef, VolumeKind,
};
pub use shapes::{BodyDef, Look, MaterialKind, RawShape, ShapeDef, ShapeKind};
pub use validation::{
    InvalidReason, MAX_COLLIDERS, MAX_CUES, MAX_INSTANCES, MAX_LAYOUTS, MAX_SHAPES, MAX_VOLUMES,
    SUPPORTED_VERSION, ValidationError,
};
