//! Typed errors for engine-destruction. Every fallible call returns one of
//! these; nothing in this crate panics. The validation helpers shared by the
//! constructors also live here so every rejection uses the same wording.

use engine_core::CoreError;
use glam::Vec3;
use thiserror::Error;

use crate::ids::{ElementId, MaterialId};

/// Errors produced by engine-destruction.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DestructionError {
    /// The element id does not name an element of this structure.
    #[error("element {0:?} is not part of this structure")]
    UnknownElement(ElementId),
    /// The material id was not returned by this builder.
    #[error("material {0:?} is not registered with this builder")]
    UnknownMaterial(MaterialId),
    /// A value that must be finite and strictly positive was not.
    #[error("`{field}` must be finite and positive, got {value}")]
    NotPositive {
        /// Name of the rejected field.
        field: &'static str,
        /// The rejected value.
        value: f32,
    },
    /// A value that must be finite was NaN or infinite.
    #[error("`{field}` must be finite")]
    NotFinite {
        /// Name of the rejected field.
        field: &'static str,
    },
    /// An edge would connect an element to itself.
    #[error("element {0:?} cannot be connected to itself")]
    SelfEdge(ElementId),
    /// The same undirected edge was declared more than once.
    #[error("the edge between {0:?} and {1:?} was declared more than once")]
    DuplicateEdge(ElementId, ElementId),
    /// The builder holds no elements.
    #[error("structure has no elements")]
    Empty,
    /// No element is an anchor, so nothing can be supported.
    #[error("structure has no anchor element")]
    NoAnchor,
    /// The element has no path of edges to any anchor.
    #[error("element {0:?} is not connected to any anchor")]
    Unanchored(ElementId),
    /// More elements than a 32-bit element id can name.
    #[error("too many elements for a 32-bit element id")]
    TooManyElements,
    /// More materials than a 32-bit material id can name.
    #[error("too many materials for a 32-bit material id")]
    TooManyMaterials,
    /// Detached group ids have run out of their 32-bit space.
    #[error("detached group ids are exhausted")]
    GroupIdsExhausted,
    /// An engine-core helper rejected its input.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// An internal invariant was violated. Reaching this is a bug in engine-destruction.
    #[error("internal invariant violated: {0}")]
    Invariant(&'static str),
}

/// Accepts a finite, strictly positive value.
pub(crate) fn require_positive(field: &'static str, value: f32) -> Result<f32, DestructionError> {
    if !value.is_finite() {
        return Err(DestructionError::NotFinite { field });
    }
    if value <= 0.0 {
        return Err(DestructionError::NotPositive { field, value });
    }
    Ok(value)
}

/// Accepts a finite value of any sign.
pub(crate) fn require_finite(field: &'static str, value: f32) -> Result<f32, DestructionError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(DestructionError::NotFinite { field })
    }
}

/// Accepts a point whose three components are all finite.
pub(crate) fn require_finite_point(
    field: &'static str,
    point: Vec3,
) -> Result<Vec3, DestructionError> {
    require_finite(field, point.x)?;
    require_finite(field, point.y)?;
    require_finite(field, point.z)?;
    Ok(point)
}

/// Bounds-checked read of a table entry. A miss means an internal table is out of
/// step with the id space, so it reports [`DestructionError::Invariant`].
pub(crate) fn at<T>(items: &[T], index: usize) -> Result<&T, DestructionError> {
    items
        .get(index)
        .ok_or(DestructionError::Invariant("table index out of range"))
}

/// Bounds-checked mutable access to a table entry; see [`at`].
pub(crate) fn at_mut<T>(items: &mut [T], index: usize) -> Result<&mut T, DestructionError> {
    items
        .get_mut(index)
        .ok_or(DestructionError::Invariant("table index out of range"))
}
