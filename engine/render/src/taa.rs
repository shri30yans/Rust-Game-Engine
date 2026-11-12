//! Pure maths for temporal anti-aliasing: the Halton sub-pixel jitter and the projection
//! offset it applies. No GPU types live here, so every rule is unit tested on the CPU.
//!
//! Invariants: [`halton`] is in [0, 1) for every base; [`Jitter`] repeats the same
//! [`JITTER_SAMPLES`] offsets in the same order, each within [-0.5, 0.5) pixels; and
//! [`jittered_projection`] moves a projected point by exactly the NDC offset it is given.

use glam::{Mat4, Vec2};

/// Samples in the jitter sequence before it repeats.
pub const JITTER_SAMPLES: u32 = 16;

/// Radical inverse of `index` in `base`: the Halton sequence value, in [0, 1).
/// A base below 2 has no sequence and yields 0.
pub fn halton(index: u32, base: u32) -> f32 {
    if base < 2 {
        return 0.0;
    }
    let inv_base = 1.0 / base as f32;
    let mut i = index;
    let mut f = 1.0_f32;
    let mut r = 0.0_f32;
    while i > 0 {
        f *= inv_base;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// The sub-pixel offset for sample `i` (0 to 15) of the (2,3) Halton sequence, in pixels.
/// Index 0 is skipped because the first Halton value is always 0.
fn sample(i: u32) -> Vec2 {
    Vec2::new(halton(i + 1, 2) - 0.5, halton(i + 1, 3) - 0.5)
}

/// Cycles through the 16-sample (2,3) Halton jitter, one sample per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Jitter {
    next: u32,
}

impl Jitter {
    /// A jitter that starts at the first sample.
    pub fn new() -> Self {
        Self { next: 0 }
    }

    /// The sub-pixel offset for this frame, in pixels within [-0.5, 0.5); advances the sequence.
    pub fn next_sample(&mut self) -> Vec2 {
        let s = sample(self.next);
        self.next = (self.next + 1) % JITTER_SAMPLES;
        s
    }

    /// Converts a pixel-unit `sample` to an NDC offset for a `width` by `height` target.
    /// A zero size is treated as one pixel so the result stays finite.
    pub fn offset_ndc(sample: Vec2, width: u32, height: u32) -> Vec2 {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        Vec2::new(sample.x * 2.0 / w, sample.y * 2.0 / h)
    }
}

/// Returns `proj` with its projected NDC position shifted by `ndc`.
///
/// For a perspective matrix the clip `w` is `-z_view`, so `clip.xy` must move by
/// `ndc * w = -ndc * z_view`. That is exactly what subtracting `ndc` from the z-column
/// x and y terms does.
pub fn jittered_projection(proj: Mat4, ndc: Vec2) -> Mat4 {
    let mut m = proj;
    m.z_axis.x -= ndc.x;
    m.z_axis.y -= ndc.y;
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn halton_base_2_matches_hand_values() {
        let want = [0.5, 0.25, 0.75, 0.125, 0.625, 0.375, 0.875, 0.0625];
        for (index, w) in (1u32..).zip(want.iter()) {
            let got = halton(index, 2);
            assert!(close(got, *w), "base 2 index {index}: {got} != {w}");
        }
    }

    #[test]
    fn halton_base_3_matches_hand_values() {
        let third = 1.0 / 3.0;
        let want = [
            third,
            2.0 * third,
            1.0 / 9.0,
            4.0 / 9.0,
            7.0 / 9.0,
            2.0 / 9.0,
            5.0 / 9.0,
            8.0 / 9.0,
        ];
        for (index, w) in (1u32..).zip(want.iter()) {
            let got = halton(index, 3);
            assert!(close(got, *w), "base 3 index {index}: {got} != {w}");
        }
    }

    #[test]
    fn halton_index_zero_and_degenerate_base_are_zero() {
        assert_eq!(halton(0, 2), 0.0);
        assert_eq!(halton(7, 0), 0.0);
        assert_eq!(halton(7, 1), 0.0);
    }

    #[test]
    fn halton_stays_in_unit_interval() {
        for base in [2, 3, 5, 7] {
            for index in 0..4096 {
                let v = halton(index, base);
                assert!((0.0..1.0).contains(&v), "halton({index}, {base}) = {v}");
            }
        }
    }

    #[test]
    fn sequence_has_sixteen_samples_all_in_range() {
        let mut j = Jitter::new();
        let mut seen = Vec::new();
        for _ in 0..JITTER_SAMPLES {
            seen.push(j.next_sample());
        }
        assert_eq!(seen.len(), 16);
        for s in &seen {
            assert!(
                (-0.5..0.5).contains(&s.x) && (-0.5..0.5).contains(&s.y),
                "sample {s} outside [-0.5, 0.5)"
            );
        }
        // The 16 samples are distinct points of the sequence, not one value repeated.
        for (a, sa) in seen.iter().enumerate() {
            for sb in seen.iter().skip(a + 1) {
                assert_ne!(sa, sb);
            }
        }
    }

    #[test]
    fn next_cycles_after_sixteen() {
        let mut j = Jitter::new();
        let first: Vec<Vec2> = (0..JITTER_SAMPLES).map(|_| j.next_sample()).collect();
        let second: Vec<Vec2> = (0..JITTER_SAMPLES).map(|_| j.next_sample()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn offset_ndc_is_two_over_size() {
        let ndc = Jitter::offset_ndc(Vec2::new(0.5, -0.25), 256, 128);
        assert!(close(ndc.x, 0.5 * 2.0 / 256.0));
        assert!(close(ndc.y, -0.25 * 2.0 / 128.0));
    }

    #[test]
    fn offset_ndc_zero_size_stays_finite() {
        let ndc = Jitter::offset_ndc(Vec2::new(0.5, 0.5), 0, 0);
        assert!(ndc.x.is_finite() && ndc.y.is_finite());
    }

    #[test]
    fn jitter_shifts_projected_point_by_exact_pixels() {
        let (w, h) = (256_u32, 192_u32);
        let proj = Mat4::perspective_rh(1.2, w as f32 / h as f32, 0.1, 100.0);
        let point = Vec4::new(0.7, -0.4, -10.0, 1.0);
        let project = |m: Mat4| {
            let c = m * point;
            Vec2::new(c.x / c.w, c.y / c.w)
        };
        let base = project(proj);
        for sample_px in [
            Vec2::new(0.25, -0.5),
            Vec2::new(-0.375, 0.125),
            Vec2::new(0.4375, 0.3125),
        ] {
            let ndc = Jitter::offset_ndc(sample_px, w, h);
            let shifted = project(jittered_projection(proj, ndc));
            let shift_px = Vec2::new(
                (shifted.x - base.x) * w as f32 * 0.5,
                (shifted.y - base.y) * h as f32 * 0.5,
            );
            assert!(
                (shift_px - sample_px).length() < 1e-4,
                "expected {sample_px} px, got {shift_px} px"
            );
        }
    }

    #[test]
    fn zero_jitter_leaves_projection_unchanged() {
        let proj = Mat4::perspective_rh(1.0, 1.5, 0.05, 500.0);
        assert_eq!(jittered_projection(proj, Vec2::ZERO), proj);
    }
}
