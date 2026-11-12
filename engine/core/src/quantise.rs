//! Uniform quantisation of `f32` values onto an integer grid.
//!
//! A [`Quantiser`] maps `[min, max]` onto the levels `0..=max_level`, where
//! `max_level = 2^bits - 1`. Encoding rounds to the nearest level and clamps
//! out-of-range input; NaN encodes as level 0.
//!
//! Error bound: for any `x` in `[min, max]`, the decoded value differs from
//! `x` by at most half a step, `(max - min) / (2 * max_level)`, which is
//! [`Quantiser::max_error`]. Decoding rounds the result to `f32`, which adds at
//! most `max(|min|, |max|) * f32::EPSILON`; that term is the only slack beyond
//! the half-step bound and is covered by the property tests.

use crate::error::CoreError;

/// Smallest supported bit width.
const MIN_BITS: u32 = 1;
/// Largest supported bit width. Levels up to 2^24 - 1 are exact in `f64`, and
/// the decoded value stays well inside `f32` precision.
const MAX_BITS: u32 = 24;

/// Uniform quantiser over a validated `[min, max]` range with `bits` bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantiser {
    min: f32,
    max: f32,
    bits: u32,
}

impl Quantiser {
    /// Builds a quantiser over `[min, max]` using `bits` bits per value.
    ///
    /// # Errors
    /// Returns [`CoreError::InvalidBounds`] unless both bounds are finite with
    /// `min < max`, and [`CoreError::InvalidBits`]
    /// unless `bits` is in `1..=24`.
    pub fn new(min: f32, max: f32, bits: u32) -> Result<Self, CoreError> {
        // Both bounds are finite, so their difference is finite even in f64 (the span of any two f32 values fits in f64).
        if !min.is_finite() || !max.is_finite() || min >= max {
            return Err(CoreError::InvalidBounds { min, max });
        }
        if !(MIN_BITS..=MAX_BITS).contains(&bits) {
            return Err(CoreError::InvalidBits { bits });
        }
        Ok(Quantiser { min, max, bits })
    }

    /// Lower bound of the encodable range.
    #[must_use]
    pub const fn min(&self) -> f32 {
        self.min
    }

    /// Upper bound of the encodable range.
    #[must_use]
    pub const fn max(&self) -> f32 {
        self.max
    }

    /// Bits per encoded value.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Largest encoded level, `2^bits - 1`.
    #[must_use]
    pub const fn max_level(&self) -> u32 {
        (1u32 << self.bits) - 1
    }

    /// Maximum absolute error for an in-range value: half a quantisation step,
    /// as an `f32`. See the module docs for the additional `f32` rounding slack.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Narrowing a half step to f32 is the documented, intended rounding.
    pub fn max_error(&self) -> f32 {
        (self.range() / (2.0 * f64::from(self.max_level()))) as f32
    }

    /// Encodes `value` to its nearest level. Out-of-range values clamp to the
    /// end levels; NaN encodes as level 0.
    #[must_use]
    pub fn encode(&self, value: f32) -> u32 {
        if value.is_nan() {
            return 0;
        }
        let min = f64::from(self.min);
        let clamped = f64::from(value).clamp(min, f64::from(self.max));
        let t = (clamped - min) / self.range();
        let scaled = (t * f64::from(self.max_level())).round();
        // `scaled` is in [0, max_level] after clamping, so the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let level = scaled.clamp(0.0, f64::from(self.max_level())) as u32;
        level
    }

    /// Decodes a level back to a value. Levels above `max_level` clamp to `max`.
    #[must_use]
    pub fn decode(&self, level: u32) -> f32 {
        let l = level.min(self.max_level());
        let t = f64::from(l) / f64::from(self.max_level());
        let value = f64::from(self.min) + self.range() * t;
        // The `f64` value is clamped into [min, max], so narrowing to `f32` stays in range.
        #[allow(clippy::cast_possible_truncation)]
        let narrowed = value.clamp(f64::from(self.min), f64::from(self.max)) as f32;
        narrowed
    }

    fn range(&self) -> f64 {
        f64::from(self.max) - f64::from(self.min)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Bound for an in-range value: half step plus the f32 rounding slack.
    fn bound_with_slack(q: &Quantiser) -> f32 {
        let abs_max = q.min().abs().max(q.max().abs());
        q.max_error() + abs_max * f32::EPSILON
    }

    #[test]
    fn rejects_invalid_bounds() {
        assert!(Quantiser::new(f32::NAN, 1.0, 8).is_err());
        assert!(Quantiser::new(0.0, f32::INFINITY, 8).is_err());
        assert!(Quantiser::new(1.0, 1.0, 8).is_err());
        assert!(Quantiser::new(2.0, 1.0, 8).is_err());
        assert!(Quantiser::new(-f32::MAX, f32::MAX, 8).is_ok());
    }

    #[test]
    fn rejects_invalid_bits() {
        assert_eq!(
            Quantiser::new(0.0, 1.0, 0),
            Err(CoreError::InvalidBits { bits: 0 })
        );
        assert_eq!(
            Quantiser::new(0.0, 1.0, 25),
            Err(CoreError::InvalidBits { bits: 25 })
        );
        assert!(Quantiser::new(0.0, 1.0, 24).is_ok());
    }

    #[test]
    fn endpoints_and_levels() {
        let q = Quantiser::new(-1.0, 1.0, 8).unwrap();
        assert_eq!(q.max_level(), 255);
        assert_eq!(q.encode(-1.0), 0);
        assert_eq!(q.encode(1.0), 255);
        assert_eq!(q.decode(0), -1.0);
        assert_eq!(q.decode(255), 1.0);
    }

    #[test]
    fn max_error_is_half_a_step() {
        let q = Quantiser::new(0.0, 1.0, 8).unwrap();
        // step = 1 / 255, half step = 1 / 510
        assert!((q.max_error() - 1.0 / 510.0).abs() < 1e-9);
    }

    #[test]
    fn nan_encodes_to_zero_and_out_of_range_clamps() {
        let q = Quantiser::new(10.0, 20.0, 4).unwrap();
        assert_eq!(q.encode(f32::NAN), 0);
        assert_eq!(q.encode(-1000.0), 0);
        assert_eq!(q.encode(1000.0), q.max_level());
        assert_eq!(q.decode(u32::MAX), 20.0);
    }

    proptest! {
        #[test]
        fn round_trip_error_within_half_step(
            min in -1.0e4f32..1.0e4,
            span in 1.0e-2f32..1.0e4,
            bits in 1u32..=24,
            t in 0.0f32..=1.0,
        ) {
            let max = min + span;
            let q = Quantiser::new(min, max, bits).unwrap();
            let x = (min + span * t).clamp(min, max);
            let back = q.decode(q.encode(x));
            prop_assert!((back - x).abs() <= bound_with_slack(&q),
                "x={x} back={back} bound={}", bound_with_slack(&q));
        }

        #[test]
        fn out_of_range_values_clamp_to_end_levels(
            min in -1.0e4f32..1.0e4,
            span in 1.0e-2f32..1.0e4,
            bits in 1u32..=24,
            overshoot in 0.0f32..1.0e5,
        ) {
            let max = min + span;
            let q = Quantiser::new(min, max, bits).unwrap();
            prop_assert_eq!(q.encode(min - overshoot), 0);
            prop_assert_eq!(q.encode(max + overshoot), q.max_level());
            let low = q.decode(q.encode(min - overshoot));
            let high = q.decode(q.encode(max + overshoot));
            prop_assert!(low >= min && low <= max);
            prop_assert!(high >= min && high <= max);
        }

        #[test]
        fn encoding_is_monotonic(
            min in -1.0e4f32..1.0e4,
            span in 1.0e-2f32..1.0e4,
            bits in 1u32..=16,
            a in 0.0f32..=1.0,
            b in 0.0f32..=1.0,
        ) {
            let max = min + span;
            let q = Quantiser::new(min, max, bits).unwrap();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let xa = min + span * lo;
            let xb = min + span * hi;
            prop_assert!(q.encode(xa) <= q.encode(xb));
        }
    }
}
