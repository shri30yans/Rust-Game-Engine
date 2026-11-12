//! Local transforms and the derived world matrices.
//!
//! [`Transform`] is authored data. Its fields are private and every constructor
//! and setter validates its input, so a stored transform is always finite with a
//! unit rotation, whether it was built in code or decoded from bytes.
//!
//! [`GlobalTransform`] is derived data. Only [`propagate_transforms`] writes it.
//! Edits made to it by hand are overwritten on the next propagation.

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::error::WorldError;
use crate::hierarchy::{Children, Parent};
use crate::net_id::NetIdAllocator;
use crate::schedule::SystemContext;
use crate::world::World;

/// Smallest squared rotation length accepted before normalisation.
const MIN_ROTATION_LENGTH_SQUARED: f32 = 1.0e-12;

/// Local translation, rotation and scale relative to the parent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TransformRaw", into = "TransformRaw")]
pub struct Transform {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

/// Wire shape of [`Transform`]. Decoding goes through validation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct TransformRaw {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl From<Transform> for TransformRaw {
    fn from(value: Transform) -> Self {
        Self {
            translation: value.translation,
            rotation: value.rotation,
            scale: value.scale,
        }
    }
}

impl TryFrom<TransformRaw> for Transform {
    type Error = WorldError;

    fn try_from(raw: TransformRaw) -> Result<Self, Self::Error> {
        Transform::new(raw.translation, raw.rotation, raw.scale)
    }
}

impl Transform {
    /// No translation, no rotation, unit scale.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Builds a transform. The rotation is normalised.
    ///
    /// # Errors
    /// Returns [`WorldError::InvalidTransform`] when a value is not finite or the
    /// rotation has zero length.
    pub fn new(translation: Vec3, rotation: Quat, scale: Vec3) -> Result<Self, WorldError> {
        Ok(Self {
            translation: check_vec(translation, "translation must be finite")?,
            rotation: check_rotation(rotation)?,
            scale: check_vec(scale, "scale must be finite")?,
        })
    }

    /// Local translation.
    #[must_use]
    pub const fn translation(&self) -> Vec3 {
        self.translation
    }

    /// Local rotation (unit length).
    #[must_use]
    pub const fn rotation(&self) -> Quat {
        self.rotation
    }

    /// Local scale.
    #[must_use]
    pub const fn scale(&self) -> Vec3 {
        self.scale
    }

    /// Replaces the translation.
    ///
    /// # Errors
    /// Returns [`WorldError::InvalidTransform`] when the value is not finite.
    pub fn set_translation(&mut self, value: Vec3) -> Result<(), WorldError> {
        self.translation = check_vec(value, "translation must be finite")?;
        Ok(())
    }

    /// Replaces the rotation. The value is normalised.
    ///
    /// # Errors
    /// Returns [`WorldError::InvalidTransform`] when the value is not finite or has
    /// zero length.
    pub fn set_rotation(&mut self, value: Quat) -> Result<(), WorldError> {
        self.rotation = check_rotation(value)?;
        Ok(())
    }

    /// Replaces the scale.
    ///
    /// # Errors
    /// Returns [`WorldError::InvalidTransform`] when the value is not finite.
    pub fn set_scale(&mut self, value: Vec3) -> Result<(), WorldError> {
        self.scale = check_vec(value, "scale must be finite")?;
        Ok(())
    }

    /// Matrix that maps this transform's local space into its parent's space.
    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

fn check_vec(value: Vec3, reason: &'static str) -> Result<Vec3, WorldError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(WorldError::InvalidTransform(reason))
    }
}

fn check_rotation(value: Quat) -> Result<Quat, WorldError> {
    if !value.is_finite() || value.length_squared() < MIN_ROTATION_LENGTH_SQUARED {
        return Err(WorldError::InvalidTransform(
            "rotation must be finite and non-zero",
        ));
    }
    Ok(value.normalize())
}

/// World-space matrix of an entity, derived from its ancestors' transforms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GlobalTransform(Mat4);

impl GlobalTransform {
    /// Identity matrix.
    pub const IDENTITY: Self = Self(Mat4::IDENTITY);

    /// World-space matrix as of the last propagation.
    #[must_use]
    pub const fn matrix(&self) -> Mat4 {
        self.0
    }

    /// World-space translation as of the last propagation.
    #[must_use]
    pub fn translation(&self) -> Vec3 {
        self.0.w_axis.truncate()
    }
}

/// System that recomputes every [`GlobalTransform`] from the hierarchy.
///
/// Roots are visited in [`NetId`](crate::NetId) order and children in insertion
/// order, so the result never depends on hecs storage layout. After the first
/// call has grown the traversal buffer, later calls perform no heap allocation.
///
/// # Errors
/// Returns [`WorldError::Invariant`] if the hierarchy is inconsistent. Mutation
/// through the world API cannot produce that state.
pub fn propagate_transforms(
    world: &mut World,
    _ctx: &mut SystemContext<'_>,
) -> Result<(), WorldError> {
    let World {
        ecs,
        ids,
        propagation_stack,
        ..
    } = world;
    propagate(ecs, ids, propagation_stack)
}

/// Depth-first walk from every root. `stack` holds `(entity, parent matrix)`.
fn propagate(
    ecs: &mut hecs::World,
    ids: &NetIdAllocator,
    stack: &mut Vec<(Entity, Mat4)>,
) -> Result<(), WorldError> {
    stack.clear();
    for (_, root) in ids.live_entities() {
        if ecs.get::<&Parent>(root).is_ok() {
            continue;
        }
        stack.push((root, Mat4::IDENTITY));
        while let Some((entity, parent_matrix)) = stack.pop() {
            let local = ecs
                .get::<&Transform>(entity)
                .map_err(|_| WorldError::Invariant("hierarchy node lacks a Transform"))?
                .matrix();
            let world_matrix = parent_matrix * local;
            {
                let mut global = ecs
                    .get::<&mut GlobalTransform>(entity)
                    .map_err(|_| WorldError::Invariant("hierarchy node lacks a GlobalTransform"))?;
                *global = GlobalTransform(world_matrix);
            }
            if let Ok(children) = ecs.get::<&Children>(entity) {
                // Pushed in reverse so the LIFO stack visits children in insertion order.
                for &child in children.list.iter().rev() {
                    let child_entity = ids.resolve(child)?;
                    stack.push((child_entity, world_matrix));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn rejects_non_finite_values() {
        assert!(Transform::new(Vec3::new(f32::NAN, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE).is_err());
        assert!(Transform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::splat(f32::INFINITY)).is_err());
        assert!(
            Transform::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 0.0), Vec3::ONE).is_err()
        );
    }

    #[test]
    fn rotation_is_normalised() {
        let t = Transform::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 2.0), Vec3::ONE).unwrap();
        assert!((t.rotation().length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn serde_round_trip_and_rejects_invalid_input() {
        let t = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_y(FRAC_PI_2),
            Vec3::ONE,
        )
        .unwrap();
        let json = serde_json::to_string(&t).unwrap();
        let back: Transform = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        let bad = r#"{"translation":[0,0,0],"rotation":[0,0,0,0],"scale":[1,1,1]}"#;
        assert!(serde_json::from_str::<Transform>(bad).is_err());
    }
}
