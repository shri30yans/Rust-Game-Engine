//! Typed errors for engine-scene. Every fallible operation returns one of these.
//! Each file-related variant carries the file name so a message points at the file to fix.

use std::io;
use std::path::PathBuf;

use engine_assets::format::AssetId;
use engine_physics::PhysicsError;
use engine_world::WorldError;
use thiserror::Error;

use crate::model::ValidationError;

/// Errors produced by engine-scene.
#[derive(Debug, Error)]
pub enum SceneError {
    /// A level or tuning file could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The text is not valid RON for the expected type (syntax, unknown field, unknown variant).
    #[error("cannot parse {file}: {source}")]
    Parse {
        /// The file name the text came from.
        file: String,
        /// The RON error, with its line and column.
        #[source]
        source: ron::error::SpannedError,
    },
    /// The text parsed but a value broke a rule. The location names the shape or marker.
    #[error("{file}: {error}")]
    Invalid {
        /// The file name the text came from.
        file: String,
        /// The refused value and where it was found.
        #[source]
        error: ValidationError,
    },
    /// A file's values were rejected by a check on the whole file: a tuning file's own validation,
    /// or a game's check on a level (the sandbox's gravity, for example).
    #[error("{file}: tuning rejected: {reason}")]
    Tuning {
        /// The file name the tuning came from.
        file: String,
        /// The validation message.
        reason: String,
    },
    /// A level could not be written as RON.
    #[error("cannot write level as RON: {0}")]
    Encode(#[from] ron::Error),
    /// The world refused a spawn or a lookup.
    #[error("world error while spawning: {0}")]
    World(#[from] WorldError),
    /// The physics world refused a validated value.
    #[error("physics error while spawning: {0}")]
    Physics(#[from] PhysicsError),
    /// A cooked asset reference could not be resolved through the stable asset index.
    #[error("cooked asset {asset_id:?}: {reason}")]
    CookedAssetReference {
        /// Stable object identifier from the level.
        asset_id: AssetId,
        /// Read, decode, lookup or shape conversion failure.
        reason: String,
    },
    /// The stable cooked-asset index could not be parsed or read.
    #[error("cannot load cooked asset index {path}: {reason}")]
    AssetIndex {
        /// Index file path.
        path: PathBuf,
        /// Read or decode failure.
        reason: String,
    },
    /// A prefab definition or component block is invalid.
    #[error("prefab {file} field `{field}`: {reason}")]
    Prefab {
        /// Prefab source file.
        file: String,
        /// Component or property name.
        field: String,
        /// Validation detail.
        reason: String,
    },
}
