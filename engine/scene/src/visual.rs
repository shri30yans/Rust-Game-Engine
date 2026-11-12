//! The visual of a scene entity: which unit mesh draws it, and how it looks.
//!
//! [`Visual`] is plain data and a registered component, so it travels with the entity. The
//! renderer side (engine-app's `ScenePresenter`) turns it into draw calls; this crate never names
//! a renderer type.

use engine_assets::format::AssetId;
use serde::{Deserialize, Serialize};

use crate::model::{Look, ShapeKind};

/// Which unit mesh draws a shape. Each mesh is built at unit size and scaled by the entity's
/// `Transform`, whose scale is the shape's full size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeshKind {
    /// A unit cube. Draws cuboids and ramps.
    Box,
    /// A unit sphere.
    Sphere,
    /// A unit capsule.
    Capsule,
    /// A cooked mesh registered by stable asset ID.
    Cooked(AssetId),
}

impl MeshKind {
    /// The mesh that draws a shape of this kind.
    #[must_use]
    pub const fn of(kind: ShapeKind) -> Self {
        match kind {
            ShapeKind::Cuboid { .. } | ShapeKind::Ramp { .. } => Self::Box,
            ShapeKind::Sphere { .. } => Self::Sphere,
            ShapeKind::Capsule { .. } => Self::Capsule,
        }
    }
}

/// How an entity is drawn: its mesh and its look.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Visual {
    /// The unit mesh.
    pub mesh: MeshKind,
    /// The look.
    pub look: Look,
}
