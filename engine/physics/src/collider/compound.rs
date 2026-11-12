//! Validated compound shape: between one and 64 convex parts at local poses.

use std::sync::Arc;

use super::convex::ConvexShape;
use crate::PhysicsError;
use crate::config::{validate_quat, validate_vec3};
use glam::{Quat, Vec3};
use rapier3d::prelude::SharedShape;

/// A list of 1 to 64 convex parts at local poses.
#[derive(Debug, Clone)]
pub struct CompoundShape {
    parts: Arc<[(Vec3, Quat, ConvexShape)]>,
}
impl CompoundShape {
    /// Creates a non-empty compound with no more than 64 convex parts.
    pub fn new(parts: Vec<(Vec3, Quat, ConvexShape)>) -> Result<Self, PhysicsError> {
        if parts.is_empty() {
            return Err(PhysicsError::EmptyCompound);
        }
        if parts.len() > 64 {
            return Err(PhysicsError::TooManyCompoundParts(parts.len()));
        }
        for (translation, rotation, _) in &parts {
            validate_vec3(*translation, "compound translation")?;
            validate_quat(*rotation, "compound rotation")?;
        }
        Ok(Self {
            parts: Arc::from(parts.into_boxed_slice()),
        })
    }
    pub(super) fn shared(&self) -> SharedShape {
        SharedShape::compound(
            self.parts
                .iter()
                .map(|(translation, rotation, shape)| {
                    (
                        crate::convert::to_rapier_pose(*translation, *rotation),
                        shape.shared(),
                    )
                })
                .collect(),
        )
    }
}
