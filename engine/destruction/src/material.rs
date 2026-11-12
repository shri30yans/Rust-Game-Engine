//! Materials: physical constants that element mass and capacity derive from.
//!
//! A material is validated when it is created, and also when it is
//! deserialised, so every material in a structure is finite and positive.

use serde::{Deserialize, Serialize};

use crate::error::{DestructionError, require_positive};

/// Physical constants shared by elements of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MaterialFields")]
pub struct Material {
    density: f32,
    strength_per_area: f32,
}

impl Material {
    /// Creates a material. Both values must be finite and strictly positive.
    ///
    /// # Errors
    /// Returns [`DestructionError::NotPositive`] or [`DestructionError::NotFinite`].
    pub fn new(density: f32, strength_per_area: f32) -> Result<Self, DestructionError> {
        Ok(Self {
            density: require_positive("density", density)?,
            strength_per_area: require_positive("strength_per_area", strength_per_area)?,
        })
    }

    /// Mass per unit volume.
    #[must_use]
    pub const fn density(self) -> f32 {
        self.density
    }

    /// Load per unit cross-section area that an element of this material can carry.
    #[must_use]
    pub const fn strength_per_area(self) -> f32 {
        self.strength_per_area
    }
}

/// Unvalidated wire form of [`Material`], checked on the way in.
#[derive(Deserialize)]
struct MaterialFields {
    density: f32,
    strength_per_area: f32,
}

impl TryFrom<MaterialFields> for Material {
    type Error = DestructionError;

    fn try_from(fields: MaterialFields) -> Result<Self, Self::Error> {
        Material::new(fields.density, fields.strength_per_area)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_and_non_finite_values() {
        assert!(matches!(
            Material::new(0.0, 1.0),
            Err(DestructionError::NotPositive {
                field: "density",
                ..
            })
        ));
        assert!(matches!(
            Material::new(1.0, f32::NAN),
            Err(DestructionError::NotFinite {
                field: "strength_per_area"
            })
        ));
        assert!(Material::new(-1.0, 1.0).is_err());
    }

    #[test]
    fn deserialise_is_validated() {
        let good: Material =
            serde_json::from_str(r#"{"density": 2.0, "strength_per_area": 3.0}"#).unwrap();
        assert_eq!(good.density(), 2.0);
        assert_eq!(good.strength_per_area(), 3.0);
        assert!(
            serde_json::from_str::<Material>(r#"{"density": -2.0, "strength_per_area": 3.0}"#)
                .is_err()
        );
    }
}
