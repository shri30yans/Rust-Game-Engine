//! Shared error type for engine-core. Constructors validate input and return
//! these instead of panicking.

use thiserror::Error;

/// Errors produced by engine-core.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CoreError {
    /// A quantiser was built with bounds that cannot encode values.
    #[error("invalid quantiser bounds: min={min}, max={max} (both must be finite with min < max)")]
    InvalidBounds {
        /// Requested lower bound.
        min: f32,
        /// Requested upper bound.
        max: f32,
    },
    /// A quantiser bit width outside the supported 1..=24 range.
    #[error("invalid quantiser bit width {bits} (supported: 1..=24)")]
    InvalidBits {
        /// Requested bit width.
        bits: u32,
    },
    /// A range helper was called with an empty or inverted range.
    #[error("empty or inverted range [{lo}, {hi})")]
    EmptyRange {
        /// Inclusive lower bound.
        lo: u32,
        /// Exclusive upper bound.
        hi: u32,
    },
    /// A float range helper was called with non-finite or inverted bounds.
    #[error("invalid float range [{lo}, {hi})")]
    InvalidFloatRange {
        /// Requested lower bound.
        lo: f32,
        /// Requested upper bound.
        hi: f32,
    },
    /// The process-wide tracing subscriber could not be installed.
    #[error("tracing initialisation failed: {0}")]
    TracingInit(String),
}
