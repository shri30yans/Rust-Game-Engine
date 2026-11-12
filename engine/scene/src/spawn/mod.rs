//! Scene spawning for primitives, cooked content, and level lifecycle.
//!
//! Child modules keep construction, ownership, and teardown separate while public items remain
//! available through the stable `spawn::` paths.

mod collider;
mod content;
mod instance;
mod level;
mod lifecycle;
mod primitives;
mod records;
mod types;

#[cfg(test)]
mod tests;

pub use content::{authored_transform, spawn_record_from_scene_content};
pub use level::{
    install_level_from_scene_content, spawn_level, spawn_level_from_content,
    spawn_level_from_content_with_prefabs, spawn_level_from_scene_content,
    spawn_level_with_metadata,
};
pub use lifecycle::{
    despawn, despawn_entry, install_level, install_level_from_content,
    install_level_from_content_with_prefabs, install_level_with_metadata, installed, installed_mut,
    move_entry,
};
pub use primitives::{register_components, spawn_shape};
pub use records::{LevelInstance, Spawned, SpawnedEntry};
pub use types::{
    ColliderMetadata, MarkerKind, Markers, SceneContent, SpawnedBody, SpawnedCollider,
};
