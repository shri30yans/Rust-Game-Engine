//! Validation helpers shared by every constructor in the kit. Each one returns the value unchanged
//! or a [`KitError`] that names the field, so no config can hold a NaN, an infinity or an
//! out-of-range number.

use glam::{Vec2, Vec3};

use crate::error::KitError;

/// Accepts any finite scalar.
pub(crate) fn finite(value: f32, field: &'static str) -> Result<f32, KitError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(KitError::NonFinite { field, value })
    }
}

/// Accepts a finite scalar strictly greater than zero.
pub(crate) fn positive(value: f32, field: &'static str) -> Result<f32, KitError> {
    let value = finite(value, field)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(KitError::OutOfRange {
            field,
            requirement: "greater than zero",
            value,
        })
    }
}

/// Accepts a finite scalar zero or greater.
pub(crate) fn non_negative(value: f32, field: &'static str) -> Result<f32, KitError> {
    let value = finite(value, field)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(KitError::OutOfRange {
            field,
            requirement: "zero or greater",
            value,
        })
    }
}

/// Accepts a finite scalar in the closed range `[0, 1]`.
pub(crate) fn fraction(value: f32, field: &'static str) -> Result<f32, KitError> {
    let value = finite(value, field)?;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(KitError::OutOfRange {
            field,
            requirement: "between zero and one inclusive",
            value,
        })
    }
}

/// Accepts an integer count of at least one.
pub(crate) fn at_least_one(value: u32, field: &'static str) -> Result<u32, KitError> {
    if value >= 1 {
        Ok(value)
    } else {
        Err(KitError::CountOutOfRange {
            field,
            requirement: "at least one",
            value,
        })
    }
}

/// Accepts a vector whose components are all finite.
pub(crate) fn finite_vec2(value: Vec2, field: &'static str) -> Result<Vec2, KitError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(KitError::NonFiniteVector { field })
    }
}

/// Accepts a 3-vector whose components are all finite.
pub(crate) fn finite_vec3(value: Vec3, field: &'static str) -> Result<Vec3, KitError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(KitError::NonFiniteVector { field })
    }
}
