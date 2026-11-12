//! Typed failures for physics configuration, mutation, and queries.

use crate::{BodyId, BodyKind, ColliderId};
use thiserror::Error;

/// A recoverable physics configuration or lookup failure.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PhysicsError {
    /// A scalar or vector contains a non-finite component.
    #[error("{field} must contain only finite values")]
    NonFinite {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A scalar lies outside of its accepted range.
    #[error("{field} must be {requirement}, got {value}")]
    InvalidValue {
        /// Name of the invalid field.
        field: &'static str,
        /// Human-readable range requirement.
        requirement: &'static str,
        /// Rejected value.
        value: f32,
    },
    /// A quaternion has no usable rotation axis.
    #[error("{field} must be a non-zero quaternion")]
    ZeroQuaternion {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A body identifier no longer names a live body.
    #[error("body {0:?} is stale")]
    StaleBody(BodyId),
    /// A collider identifier no longer names a live collider.
    #[error("collider {0:?} is stale")]
    StaleCollider(ColliderId),
    /// An operation was requested for a body with the wrong simulation kind.
    #[error("body {body:?} must be {required:?}")]
    WrongBodyKind {
        /// Body that rejected the operation.
        body: BodyId,
        /// Body kind required by the operation.
        required: BodyKind,
    },
    /// A convex hull could not be formed from the supplied points.
    #[error("convex hull points are degenerate")]
    DegenerateHull,
    /// A triangle mesh has invalid counts, indices or topology values.
    #[error("triangle mesh is invalid")]
    InvalidMesh,
    /// A triangle has zero area.
    #[error("triangle mesh contains a degenerate triangle")]
    DegenerateTriangle,
    /// A compound shape has no parts.
    #[error("compound shape must have at least one part")]
    EmptyCompound,
    /// A compound shape exceeds Rapier wrapper's part limit.
    #[error("compound shape has {0} parts; maximum is 64")]
    TooManyCompoundParts(usize),
    /// Convex decomposition settings were outside their supported range.
    #[error("invalid convex decomposition parameters")]
    InvalidDecompositionParams,
    /// Triangle mesh colliders must not be attached to dynamic bodies.
    #[error("triangle mesh collider cannot be attached to a dynamic body")]
    DynamicTriangleMesh,
}

pub(crate) fn finite(value: f32, field: &'static str) -> Result<(), PhysicsError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(PhysicsError::NonFinite { field })
    }
}

pub(crate) fn positive(value: f32, field: &'static str) -> Result<(), PhysicsError> {
    finite(value, field)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(PhysicsError::InvalidValue {
            field,
            requirement: "greater than zero",
            value,
        })
    }
}

pub(crate) fn non_negative(value: f32, field: &'static str) -> Result<(), PhysicsError> {
    finite(value, field)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(PhysicsError::InvalidValue {
            field,
            requirement: "greater than or equal to zero",
            value,
        })
    }
}
