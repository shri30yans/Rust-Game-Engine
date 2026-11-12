//! Shared level limits, validation errors, and scalar checks.
use thiserror::Error;

/// The only level file version this engine reads. Any other version is refused, not guessed at.
pub const SUPPORTED_VERSION: u32 = 1;

/// The most shapes one level may contain. The limit keeps a runaway file from exhausting the
/// entity space; it is well above any level the engine is tuned for.
pub const MAX_SHAPES: usize = 512;
/// Maximum retained mesh instances in one level.
pub const MAX_INSTANCES: usize = 4096;
/// Maximum independent cooked collision sets in one level.
pub const MAX_COLLIDERS: usize = 4096;
/// Maximum tagged volumes in one level.
pub const MAX_VOLUMES: usize = 1024;
/// Maximum layouts stored in one level.
pub const MAX_LAYOUTS: usize = 32;
/// Maximum named Director cues in one level.
pub const MAX_CUES: usize = 256;

/// Why a value was refused.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum InvalidReason {
    /// The file's version is not the one this engine reads.
    #[error("unsupported level version {found}, this engine reads version {expected}")]
    Version {
        /// The version in the file.
        found: u32,
        /// The version this engine reads.
        expected: u32,
    },
    /// A number is NaN or infinite.
    #[error("{field} is not a finite number")]
    NonFinite {
        /// The field that holds it.
        field: &'static str,
    },
    /// A number that must be above zero is zero or negative.
    #[error("{field} must be greater than zero")]
    NotPositive {
        /// The field that holds it.
        field: &'static str,
    },
    /// A value outside `[0, 1]`.
    #[error("{field} must be between 0 and 1")]
    OutsideUnit {
        /// The field that holds it.
        field: &'static str,
    },
    /// A name is empty.
    #[error("name must not be empty")]
    EmptyName,
    /// A layout or Director cue name is not lower_snake_case.
    #[error("name must use lower_snake_case")]
    InvalidNameFormat,
    /// A name appears twice in the same list.
    #[error("name `{0}` is used more than once")]
    DuplicateName(String),
    /// A cue refers to a layout that does not exist.
    #[error("cue refers to unknown layout `{0}`")]
    UnknownLayout(String),
    /// Layouts do not describe the same mover and switch ID sets.
    #[error("layout has a different mover or switch ID set than the other layouts")]
    LayoutObjectsMismatch,
    /// A colour is wider than 24 bits.
    #[error("colour {0:#x} does not fit in 24 bits")]
    WideColour(u32),
    /// The level has more shapes than the engine allows.
    #[error("{count} shapes exceed the limit of {limit}")]
    TooManyShapes {
        /// Shapes in the file.
        count: usize,
        /// The limit, [`MAX_SHAPES`].
        limit: usize,
    },
    /// A level record list exceeds its configured bound.
    #[error("{field} count {count} exceeds the limit of {limit}")]
    TooManyRecords {
        /// Which record list exceeded its limit.
        field: &'static str,
        /// Number of records in the file.
        count: usize,
        /// The list's supported maximum.
        limit: usize,
    },
}

/// A refused value and where it was found, for example `shape 3 ("crate")`.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{location}: {reason}")]
pub struct ValidationError {
    /// Where the value is in the file.
    pub location: String,
    /// The rule it broke.
    pub reason: InvalidReason,
}

impl ValidationError {
    /// Builds an error for `reason` at `location`.
    #[must_use]
    pub fn new(location: impl Into<String>, reason: InvalidReason) -> Self {
        Self {
            location: location.into(),
            reason,
        }
    }
}

pub(super) fn finite(field: &'static str, values: &[f32]) -> Result<(), InvalidReason> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(InvalidReason::NonFinite { field })
    }
}

pub(super) fn positive(field: &'static str, values: &[f32]) -> Result<(), InvalidReason> {
    finite(field, values)?;
    if values.iter().all(|v| *v > 0.0) {
        Ok(())
    } else {
        Err(InvalidReason::NotPositive { field })
    }
}

pub(super) fn unit(field: &'static str, value: f32) -> Result<(), InvalidReason> {
    finite(field, &[value])?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(InvalidReason::OutsideUnit { field })
    }
}

pub(super) fn non_empty(value: &str) -> Result<(), InvalidReason> {
    if value.is_empty() {
        Err(InvalidReason::EmptyName)
    } else {
        Ok(())
    }
}
