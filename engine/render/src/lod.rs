//! Screen-size LOD selection shared by camera and shadow culling.
//!
//! LOD indices increase as detail decreases. The selector keeps the previous index until the
//! projected size crosses a ten-percent hysteresis band, which prevents threshold chatter.

/// Fractional margin applied to LOD screen-size thresholds.
pub const LOD_HYSTERESIS: f32 = 0.10;

/// Selects an LOD from largest to smallest projected screen size.
///
/// `thresholds` are ordered from finest to coarsest and must be non-increasing. A higher screen
/// size selects a finer LOD. Invalid inputs return the coarsest valid LOD (or zero for no LODs).
#[must_use]
pub fn select_lod(screen_size: f32, thresholds: &[f32], previous: usize) -> usize {
    if thresholds.is_empty() {
        return 0;
    }
    let last = thresholds.len().saturating_sub(1);
    let previous = previous.min(last);
    if !screen_size.is_finite() || screen_size < 0.0 {
        return last;
    }

    let mut selected = last;
    for (index, threshold) in thresholds.iter().copied().enumerate() {
        if !threshold.is_finite() || threshold < 0.0 {
            continue;
        }
        let boundary = if index < previous {
            threshold * (1.0 + LOD_HYSTERESIS)
        } else {
            threshold * (1.0 - LOD_HYSTERESIS)
        };
        if screen_size >= boundary {
            selected = index;
            break;
        }
    }
    selected
}

/// Returns the camera LOD with one additional coarser step for a shadow cascade.
#[must_use]
pub fn shadow_lod(camera_lod: usize, lod_count: usize) -> usize {
    camera_lod
        .saturating_add(1)
        .min(lod_count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const THRESHOLDS: [f32; 3] = [0.5, 0.25, 0.0];

    #[test]
    fn selection_holds_in_the_hysteresis_band() {
        assert_eq!(select_lod(0.51, &THRESHOLDS, 0), 0);
        assert_eq!(select_lod(0.46, &THRESHOLDS, 0), 0);
        assert_eq!(select_lod(0.44, &THRESHOLDS, 0), 1);
        assert_eq!(select_lod(0.44, &THRESHOLDS, 1), 1);
        assert_eq!(select_lod(0.54, &THRESHOLDS, 1), 1);
        assert_eq!(select_lod(0.56, &THRESHOLDS, 1), 0);
        assert_eq!(select_lod(0.56, &THRESHOLDS, 0), 0);
    }

    #[test]
    fn invalid_sizes_choose_coarsest_and_shadow_steps_down_once() {
        assert_eq!(select_lod(f32::NAN, &THRESHOLDS, 0), 2);
        assert_eq!(select_lod(0.5, &[], 0), 0);
        assert_eq!(shadow_lod(0, 3), 1);
        assert_eq!(shadow_lod(2, 3), 2);
        assert_eq!(shadow_lod(0, 0), 0);
    }

    proptest! {
        #[test]
        fn selected_lod_is_monotonic_with_distance(distance_a in 1.0f32..1000.0, gap in 0.0f32..1000.0) {
            let distance_b = distance_a + gap;
            let radius = 1.0f32;
            let screen_a = radius / distance_a;
            let screen_b = radius / distance_b;
            let lod_a = select_lod(screen_a, &THRESHOLDS, 2);
            let lod_b = select_lod(screen_b, &THRESHOLDS, 2);
            prop_assert!(lod_b >= lod_a);
        }
    }
}
