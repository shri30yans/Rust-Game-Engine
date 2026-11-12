//! Engine scene: levels and tuning as validated RON data files.
//!
//! A level lists shapes, materials, spawn points, waypoints and named markers. One loader reads
//! it from a file or embedded text, validates it into a [`Level`] that cannot hold a bad value,
//! and [`spawn::install_level`] creates the entities, physics bodies and visuals. Tuning values
//! use the same loader. In development builds [`watch::Watched`] reloads the file when it changes,
//! and a bad edit keeps the old value running.
//!
//! Invariants: a validated [`Level`] or [`model::ShapeDef`] cannot hold a non-finite number, a
//! non-positive extent or an unknown version; a failed reload never replaces the running value;
//! the crate has no render dependency, so the renderer reads [`visual::Visual`] as plain data.
#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod prefab;
pub mod spawn;
pub mod validate;
pub mod visual;
pub mod visual_assets;
pub mod watch;

pub use engine_assets::format::AssetId;
pub use error::SceneError;
pub use model::{
    BodyDef, CookedColliderDef, CueDef, InstanceCollision, InstanceDef, InvalidReason, LayoutDef,
    Level, LevelRecord, LevelRecordDef, LevelTransform, Look, MAX_COLLIDERS, MAX_CUES,
    MAX_INSTANCES, MAX_LAYOUTS, MAX_SHAPES, MAX_VOLUMES, Marker, MaterialKind, PrefabPlacement,
    RawLevel, RawShape, SUPPORTED_VERSION, ShapeDef, ShapeKind, ValidationError, VolumeDef,
    VolumeKind,
};
pub use prefab::{PrefabCollision, PrefabDef, PrefabRegistry, PrefabSpawn, parse_prefab};
pub use spawn::{
    ColliderMetadata, LevelInstance, MarkerKind, Markers, SceneContent, Spawned, SpawnedBody,
    SpawnedCollider, SpawnedEntry, authored_transform, despawn, despawn_entry, install_level,
    install_level_from_content, install_level_from_content_with_prefabs,
    install_level_from_scene_content, install_level_with_metadata, installed, installed_mut,
    move_entry, register_components, spawn_level, spawn_level_from_content,
    spawn_level_from_content_with_prefabs, spawn_level_from_scene_content,
    spawn_level_with_metadata, spawn_record_from_scene_content, spawn_shape,
};
pub use validate::Validate;
pub use visual::{MeshKind, Visual};
pub use visual_assets::{CookedVisualAsset, resolve_visual_assets};
pub use watch::{Reload, Watched, WatchedLevel};
