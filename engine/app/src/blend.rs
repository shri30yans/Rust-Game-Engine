//! Blending: mixes the transforms of the last two ticks for drawing between them.
//!
//! Pure maths with no GPU and no clock. Translation and scale are interpolated linearly,
//! rotation along the shortest arc (slerp). Invariant: the result is always a valid
//! [`Transform`], because it is built through [`Transform::new`]; an `alpha` outside
//! `[0, 1]` (or NaN) is rejected before any maths runs.

use engine_world::Transform;

use crate::error::AppError;

/// Blends two transforms: translation and scale by lerp, rotation by slerp.
///
/// `alpha` 0 gives `previous`, 1 gives `current`.
///
/// # Errors
///
/// Returns [`AppError::BlendAlpha`] when `alpha` is outside `[0, 1]` or NaN, and
/// [`AppError::World`] when the blended value is not a valid transform.
pub fn blend(previous: &Transform, current: &Transform, alpha: f32) -> Result<Transform, AppError> {
    if !(0.0..=1.0).contains(&alpha) {
        return Err(AppError::BlendAlpha(alpha));
    }
    let translation = previous.translation().lerp(current.translation(), alpha);
    let rotation = previous.rotation().slerp(current.rotation(), alpha);
    let scale = previous.scale().lerp(current.scale(), alpha);
    Transform::new(translation, rotation, scale).map_err(AppError::World)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use glam::{Quat, Vec3};
    use proptest::prelude::*;

    fn at(translation: Vec3, rotation: Quat, scale: Vec3) -> Transform {
        Transform::new(translation, rotation, scale).expect("valid transform")
    }

    fn assert_same(a: &Transform, b: &Transform) {
        assert!(a.translation().abs_diff_eq(b.translation(), 1e-5));
        assert!(a.scale().abs_diff_eq(b.scale(), 1e-5));
        assert!(a.rotation().dot(b.rotation()).abs() > 1.0 - 1e-5);
    }

    #[test]
    fn alpha_zero_gives_previous() {
        let previous = at(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY, Vec3::splat(2.0));
        let current = at(
            Vec3::new(-4.0, 0.0, 9.0),
            Quat::from_rotation_y(1.0),
            Vec3::ONE,
        );
        let blended = blend(&previous, &current, 0.0).expect("alpha 0 is valid");
        assert_same(&blended, &previous);
    }

    #[test]
    fn alpha_one_gives_current() {
        let previous = at(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        let current = at(
            Vec3::new(5.0, -1.0, 0.5),
            Quat::from_rotation_x(0.7),
            Vec3::new(1.0, 3.0, 1.0),
        );
        let blended = blend(&previous, &current, 1.0).expect("alpha 1 is valid");
        assert_same(&blended, &current);
    }

    #[test]
    fn midpoint_of_two_translations_is_their_average() {
        let previous = at(Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE);
        let current = at(Vec3::new(2.0, 4.0, -6.0), Quat::IDENTITY, Vec3::ONE);
        let blended = blend(&previous, &current, 0.5).expect("midpoint");
        assert!(
            blended
                .translation()
                .abs_diff_eq(Vec3::new(1.0, 2.0, -3.0), 1e-6)
        );
    }

    #[test]
    fn slerp_halfway_between_two_turns_is_half_the_turn() {
        let previous = at(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        let current = at(
            Vec3::ZERO,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::ONE,
        );
        let blended = blend(&previous, &current, 0.5).expect("midpoint");
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(blended.rotation().dot(expected).abs() > 1.0 - 1e-5);
        assert!((blended.rotation().length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn alpha_outside_the_unit_range_is_an_error() {
        let t = at(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        for bad in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(blend(&t, &t, bad), Err(AppError::BlendAlpha(_))),
                "alpha {bad} must be rejected"
            );
        }
    }

    fn finite_transform() -> impl Strategy<Value = Transform> {
        (
            prop::array::uniform3(-1000.0f32..1000.0),
            prop::array::uniform4(-1.0f32..1.0),
            prop::array::uniform3(0.01f32..100.0),
        )
            .prop_filter_map("rotation must not be zero", |(t, q, s)| {
                let rotation = Quat::from_array(q);
                if rotation.length() < 1e-3 {
                    return None;
                }
                Transform::new(Vec3::from_array(t), rotation, Vec3::from_array(s)).ok()
            })
    }

    proptest! {
        #[test]
        fn any_alpha_in_range_gives_a_valid_transform(
            previous in finite_transform(),
            current in finite_transform(),
            alpha in 0.0f32..=1.0,
        ) {
            let blended = blend(&previous, &current, alpha).expect("alpha in range");
            prop_assert!(blended.translation().is_finite());
            prop_assert!(blended.scale().is_finite());
            prop_assert!((blended.rotation().length() - 1.0).abs() < 1e-4);
        }
    }
}
