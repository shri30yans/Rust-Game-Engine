//! The public results of an upload: a ready pack's GPU handles, the status a handle reports, and
//! the errors a pack can fail with.

use render::{MaterialId, MeshId};
use thiserror::Error;

/// GPU resources for one cooked mesh pack, aligned with its CPU mesh and material arrays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuPack {
    /// Cooked source revision that produced these resources.
    pub source_hash: u64,
    /// Uploaded meshes in the same order as `MeshPack::meshes`.
    pub meshes: Vec<MeshId>,
    /// Uploaded materials in the same order as `MeshPack::materials`.
    pub materials: Vec<MaterialId>,
}

/// GPU upload state for an asset handle.
#[derive(Debug)]
pub enum UploadStatus<'a> {
    /// The store asset or one of its GPU resources is still being prepared or uploaded.
    Pending,
    /// Every mesh and material in this pack is ready to use.
    Ready(&'a GpuPack),
    /// The pack could not be uploaded; the error names any missing texture.
    Failed(&'a UploadError),
}

/// Failure while loading or constructing a cooked GPU pack.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UploadError {
    /// A listed texture could not be found or loaded.
    #[error("texture {texture} failed to load: {reason}")]
    Texture {
        /// Content hash or display name of the missing texture.
        texture: String,
        /// Store or transcoder failure.
        reason: String,
    },
    /// The pack cannot be represented by GPU resources.
    #[error("GPU pack upload failed: {0}")]
    Render(String),
    /// A hot reload changed the number of meshes or materials.
    #[error(
        "reload changed pack counts (meshes {old_meshes} -> {new_meshes}, materials {old_materials} -> {new_materials})"
    )]
    CountMismatch {
        /// Mesh count in the last good pack.
        old_meshes: usize,
        /// Mesh count in the replacement pack.
        new_meshes: usize,
        /// Material count in the last good pack.
        old_materials: usize,
        /// Material count in the replacement pack.
        new_materials: usize,
    },
    /// The store handle has no matching upload state.
    #[error("asset has no upload state")]
    UnknownAsset,
}
