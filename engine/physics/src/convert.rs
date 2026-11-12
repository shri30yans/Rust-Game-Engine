//! Conversion between workspace `glam` types and Rapier's `glamx` math types.
//!
//! Rapier 0.36 uses glam 0.33 through `glamx`; the rest of the engine stays on glam 0.30.
//! Every cross-boundary conversion lives here so no other module names Rapier math types.

use glam::{Quat, Vec3};
use rapier3d::math::{Pose, Rotation, Vector};

/// Maps a workspace vector into Rapier's internal representation.
#[must_use]
pub(crate) fn to_rapier_vec(value: Vec3) -> Vector {
    Vector::new(value.x, value.y, value.z)
}

/// Maps a Rapier vector into workspace `glam`.
#[must_use]
pub(crate) fn from_rapier_vec(value: Vector) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

/// Builds a Rapier pose from workspace translation and rotation.
#[must_use]
pub(crate) fn to_rapier_pose(translation: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(
        to_rapier_vec(translation),
        Rotation::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
    )
}

/// Splits a Rapier pose into workspace translation and rotation.
#[must_use]
pub(crate) fn from_rapier_pose(pose: &Pose) -> (Vec3, Quat) {
    let translation = from_rapier_vec(pose.translation);
    let rotation = pose.rotation;
    (
        translation,
        Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn quats_equivalent(a: Quat, b: Quat) -> bool {
        let same = (a.x - b.x).abs() < 1.0e-6
            && (a.y - b.y).abs() < 1.0e-6
            && (a.z - b.z).abs() < 1.0e-6
            && (a.w - b.w).abs() < 1.0e-6;
        let opposite = (a.x + b.x).abs() < 1.0e-6
            && (a.y + b.y).abs() < 1.0e-6
            && (a.z + b.z).abs() < 1.0e-6
            && (a.w + b.w).abs() < 1.0e-6;
        same || opposite
    }

    proptest! {
        #[test]
        fn vec3_round_trip(x in -1.0e6f32..1.0e6f32, y in -1.0e6f32..1.0e6f32, z in -1.0e6f32..1.0e6f32) {
            prop_assume!(x.is_finite() && y.is_finite() && z.is_finite());
            let original = Vec3::new(x, y, z);
            let round_trip = from_rapier_vec(to_rapier_vec(original));
            prop_assert_eq!(original, round_trip);
        }

        #[test]
        fn pose_round_trip(
            tx in -1.0e3f32..1.0e3f32,
            ty in -1.0e3f32..1.0e3f32,
            tz in -1.0e3f32..1.0e3f32,
            qx in -1.0f32..1.0f32,
            qy in -1.0f32..1.0f32,
            qz in -1.0f32..1.0f32,
            qw in -1.0f32..1.0f32,
        ) {
            prop_assume!(tx.is_finite() && ty.is_finite() && tz.is_finite());
            let length_sq = qx * qx + qy * qy + qz * qz + qw * qw;
            prop_assume!(length_sq.is_finite() && length_sq > f32::EPSILON);
            let scale = length_sq.sqrt().recip();
            let rotation = Quat::from_xyzw(qx * scale, qy * scale, qz * scale, qw * scale);
            let translation = Vec3::new(tx, ty, tz);
            let pose = to_rapier_pose(translation, rotation);
            let (back_translation, back_rotation) = from_rapier_pose(&pose);
            prop_assert_eq!(translation, back_translation);
            prop_assert!(quats_equivalent(rotation, back_rotation));
        }
    }
}
