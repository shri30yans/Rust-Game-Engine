//! Typed failures shared by the cooker modules.
//!
//! Errors retain the distinction between malformed source data and failed output operations.
use thiserror::Error;

/// Cooker command failure.
#[derive(Debug, Error)]
pub enum CookError {
    /// Expected source and output directories.
    #[error(
        "usage: cook <game-name> | cook --watch <game-name> | cook <source-directory> <output-directory>"
    )]
    Usage,
    /// A file operation failed.
    #[error("cook I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// A glTF file could not be imported.
    #[error("invalid glTF input: {0}")]
    Gltf(#[from] gltf::Error),
    /// A decoded image is unsupported or invalid.
    #[error("invalid texture image")]
    Image,
    /// A mesh contains unsupported or malformed data.
    #[error("invalid mesh data")]
    Mesh,
    /// Collision data or a collision tag was malformed for this object.
    #[error("invalid collision data for object `{object}`: {reason}")]
    Collision {
        /// Name of the source object.
        object: String,
        /// Why its collision geometry could not be cooked.
        reason: String,
    },
    /// Two source objects claim the same persistent content id.
    #[error("duplicate asset_id {asset_id:#018x} on `{first}` and `{second}`")]
    DuplicateAssetId {
        /// Repeated stable id.
        asset_id: u64,
        /// First object in sorted pack/node order.
        first: String,
        /// Later object with the same id.
        second: String,
    },
    /// A level record refers to an ID absent from the cooked stable index.
    #[error("level {object} references missing asset_id {asset_id:#018x}")]
    MissingReference {
        /// Level record kind that contains the reference.
        object: String,
        /// Stable ID that could not be resolved.
        asset_id: u64,
    },
    /// A level entity's prefab reference does not resolve to a cooked prefab data file.
    #[error("level `{level}` entity `{entity}` references missing prefab {prefab_id:#018x}")]
    MissingPrefabReference {
        /// Level being cooked.
        level: String,
        /// Source entity that owns the prefab placement.
        entity: String,
        /// Unresolved prefab stable ID.
        prefab_id: u64,
    },
    /// A generated Level failed the engine's shared validation rules.
    #[error("generated level is invalid: {0}")]
    Level(String),
    /// Asset serialization failed.
    #[error(transparent)]
    Asset(#[from] engine_assets::AssetError),
    /// Logical content pack encoding failed.
    #[error(transparent)]
    Pack(#[from] engine_assets::PackError),
    /// A cooked pack could not be loaded while aggregating a Level.
    #[error(transparent)]
    Load(#[from] engine_assets::load::LoadError),
    /// The encoder could not create a KTX2 file.
    #[error("Basis UASTC encoding failed")]
    Encode,
    /// The encoder library could not be initialized.
    #[error("Basis UASTC encoder initialization failed")]
    EncoderInit,
    /// A report could not be serialized.
    #[error("report serialization failed: {0}")]
    Report(String),
    /// The local mesh and texture worker pool could not be created.
    #[error("cook worker pool could not be created")]
    WorkerPool,
}
