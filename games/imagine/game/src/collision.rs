//! IMAGINE's collision channel and surface table.
//!
//! The engine sees only masks and surface IDs; this module owns their game meanings.

use engine_physics::{CollisionLayers, LayerMask, QueryFilter, SurfaceId};
use engine_scene::MaterialKind;

const WORLD_BIT: u32 = 1 << 0;
const CHARACTER_BIT: u32 = 1 << 1;
const PROP_BIT: u32 = 1 << 2;
const DEBRIS_BIT: u32 = 1 << 3;
const TRIGGER_BIT: u32 = 1 << 4;
const PROJECTILE_BIT: u32 = 1 << 5;

/// Game collision channels mapped once to stable bits.
pub const WORLD: LayerMask = LayerMask::from_bits(WORLD_BIT);
/// Character channel.
pub const CHARACTER: LayerMask = LayerMask::from_bits(CHARACTER_BIT);
/// Prop channel.
pub const PROP: LayerMask = LayerMask::from_bits(PROP_BIT);
/// Debris channel.
pub const DEBRIS: LayerMask = LayerMask::from_bits(DEBRIS_BIT);
/// Trigger channel.
pub const TRIGGER: LayerMask = LayerMask::from_bits(TRIGGER_BIT);
/// Projectile channel.
pub const PROJECTILE: LayerMask = LayerMask::from_bits(PROJECTILE_BIT);

/// Layers considered by character movement, ground and mantle queries.
#[must_use]
pub const fn character_query() -> QueryFilter {
    QueryFilter {
        layers: LayerMask::from_bits(WORLD_BIT | PROP_BIT | CHARACTER_BIT),
        exclude_body: None,
        include_sensors: false,
    }
}

/// Layers considered by weapon traces.
#[must_use]
pub const fn projectile_query() -> QueryFilter {
    QueryFilter {
        layers: LayerMask::from_bits(WORLD_BIT | CHARACTER_BIT | PROP_BIT),
        exclude_body: None,
        include_sensors: false,
    }
}

/// Layers considered by the static arena sightline test.
#[must_use]
pub const fn sightline_query() -> QueryFilter {
    QueryFilter {
        layers: WORLD,
        exclude_body: None,
        include_sensors: false,
    }
}

/// Collision channels assigned to static level geometry.
#[must_use]
pub const fn world_layers() -> CollisionLayers {
    CollisionLayers {
        member: WORLD,
        collides_with: LayerMask::from_bits(CHARACTER_BIT | PROP_BIT | DEBRIS_BIT | PROJECTILE_BIT),
    }
}

/// Collision channels assigned to characters.
#[must_use]
pub const fn character_layers() -> CollisionLayers {
    CollisionLayers {
        member: CHARACTER,
        collides_with: LayerMask::from_bits(WORLD_BIT | CHARACTER_BIT | PROP_BIT | PROJECTILE_BIT),
    }
}

/// Collision channels assigned to props.
#[must_use]
pub const fn prop_layers() -> CollisionLayers {
    CollisionLayers {
        member: PROP,
        collides_with: LayerMask::from_bits(WORLD_BIT | CHARACTER_BIT | PROP_BIT | PROJECTILE_BIT),
    }
}

/// Collision channel for sensor triggers.
#[must_use]
pub const fn trigger_layers() -> CollisionLayers {
    CollisionLayers {
        member: TRIGGER,
        collides_with: LayerMask::ALL,
    }
}

/// Maps greybox material kinds to the game's physical surface IDs.
#[must_use]
pub const fn surface(material: MaterialKind) -> SurfaceId {
    match material {
        MaterialKind::Floor | MaterialKind::Concrete => SurfaceId(1),
        MaterialKind::Wall | MaterialKind::Brick => SurfaceId(2),
        MaterialKind::Wood | MaterialKind::Crate => SurfaceId(3),
        MaterialKind::Metal => SurfaceId(4),
        MaterialKind::Glass => SurfaceId(5),
        MaterialKind::Emissive | MaterialKind::Custom { .. } => SurfaceId::DEFAULT,
    }
}

/// Maps a cooked Blender material tag to the game's surface table.
#[must_use]
pub fn surface_name(name: &str) -> SurfaceId {
    match name {
        "concrete" | "concrete_floor" => SurfaceId(1),
        "brick" | "plaster" => SurfaceId(2),
        "wood" => SurfaceId(3),
        "steel" | "metal_corrugated" => SurfaceId(4),
        "glass" => SurfaceId(5),
        _ => SurfaceId::DEFAULT,
    }
}
