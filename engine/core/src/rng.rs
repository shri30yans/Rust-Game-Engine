//! Seeded deterministic random generator for simulation state.
//!
//! The core is PCG32 (XSH-RR, 64-bit state, 32-bit output), which is small,
//! serialisable and has well-defined streams. Each generator carries a
//! `key` derived from its seed and stream at construction; [`SimRng::fork`]
//! uses only that key and the requested stream id, so a forked stream does
//! not depend on how many numbers the parent has already produced. There is
//! no global state and no OS entropy: the same seed always yields the same
//! sequence on every platform.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// PCG 64-bit LCG multiplier.
const PCG_MULT: u64 = 6_364_136_223_846_793_005;
/// Golden-ratio increment used to decorrelate the key derivation.
const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
/// Scale that maps a 24-bit integer onto [0, 1) exactly.
const F32_SCALE: f32 = 1.0 / 16_777_216.0;

/// Deterministic PCG32 generator with independent forkable streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimRng {
    state: u64,
    inc: u64,
    key: u64,
}

impl SimRng {
    /// Creates a generator from a seed and a stream id. Different stream ids
    /// with the same seed produce unrelated sequences.
    #[must_use]
    pub fn new(seed: u64, stream: u64) -> Self {
        let inc = (stream << 1) | 1;
        let mut rng = SimRng {
            state: 0,
            inc,
            key: mix(seed, stream),
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    /// Derives an independent child generator for `stream_id`. The child
    /// depends only on this generator's seed-derived key and `stream_id`,
    /// never on how many values have been drawn from `self`.
    #[must_use]
    pub fn fork(&self, stream_id: u64) -> Self {
        SimRng::new(splitmix64(self.key ^ splitmix64(stream_id)), stream_id)
    }

    /// Returns the next 32 random bits.
    #[allow(clippy::cast_possible_truncation)] // Keeps the low 32 bits on purpose, as the reference PCG does; `rot` is below 32.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        // XSH-RR output permutation.
        let xorshifted = ((((old >> 18) ^ old) >> 27) & 0xffff_ffff) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Returns the next 64 random bits (two 32-bit draws, high word first).
    pub fn next_u64(&mut self) -> u64 {
        let hi = u64::from(self.next_u32());
        let lo = u64::from(self.next_u32());
        (hi << 32) | lo
    }

    /// Returns a uniform `f32` in `[0, 1)` with 24 bits of randomness.
    pub fn next_f32(&mut self) -> f32 {
        // The shifted value is below 2^24, which f32 represents exactly.
        ((self.next_u32() >> 8) as f32) * F32_SCALE
    }

    /// Returns a uniform `u32` in `[lo, hi)` without modulo bias.
    ///
    /// # Errors
    /// Returns [`CoreError::EmptyRange`] when `lo >= hi`.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> Result<u32, CoreError> {
        if lo >= hi {
            return Err(CoreError::EmptyRange { lo, hi });
        }
        let bound = hi - lo;
        // Rejecting draws below this threshold leaves a whole multiple of
        // `bound` values, so the remainder is uniform.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return Ok(lo + r % bound);
            }
        }
    }

    /// Returns a uniform `f32` in `[lo, hi)`.
    ///
    /// # Errors
    /// Returns [`CoreError::InvalidFloatRange`] when a bound is non-finite or
    /// `lo >= hi`.
    #[allow(clippy::cast_possible_truncation)] // f64 -> f32 rounding is intended; the result is clamped to [lo, hi) below.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> Result<f32, CoreError> {
        if !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return Err(CoreError::InvalidFloatRange { lo, hi });
        }
        let u = f64::from(self.next_f32());
        let value = (f64::from(lo) + u * (f64::from(hi) - f64::from(lo))) as f32;
        // Rounding to f32 can land exactly on `hi`; step back one ulp so the
        // result stays in the half-open range.
        if value >= hi {
            Ok(hi.next_down())
        } else {
            Ok(value.max(lo))
        }
    }

    /// Advances the 64-bit LCG state.
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }
}

/// Mixes a seed and a stream id into a 64-bit key.
fn mix(seed: u64, stream: u64) -> u64 {
    splitmix64(seed ^ splitmix64(stream.wrapping_add(GOLDEN)))
}

/// SplitMix64 finaliser: a bijective 64-bit mixer used only for key derivation.
fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(GOLDEN);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Reference PCG32 output for `pcg32_srandom_r(42, 54)` from the official
    /// pcg-c demo. Pins the generator so any change is caught.
    const GOLDEN_SEED42_STREAM54: [u32; 6] = [
        0xa15c_02b7,
        0x7b47_f409,
        0xba1d_3330,
        0x83d2_f293,
        0xbfa4_784b,
        0xcbed_606e,
    ];

    #[test]
    fn golden_vector_seed_42_stream_54() {
        let mut rng = SimRng::new(42, 54);
        let got: Vec<u32> = (0..GOLDEN_SEED42_STREAM54.len())
            .map(|_| rng.next_u32())
            .collect();
        assert_eq!(got, GOLDEN_SEED42_STREAM54);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = SimRng::new(7, 1);
        let mut b = SimRng::new(7, 1);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn fork_is_reproducible_and_position_independent() {
        let mut parent = SimRng::new(9, 3);
        let early = parent.fork(5);
        for _ in 0..100 {
            parent.next_u32();
        }
        let late = parent.fork(5);
        assert_eq!(early, late);
    }

    #[test]
    fn forks_differ_by_stream_id() {
        let root = SimRng::new(123, 0);
        let mut a = root.fork(1);
        let mut b = root.fork(2);
        let sa: Vec<u32> = (0..16).map(|_| a.next_u32()).collect();
        let sb: Vec<u32> = (0..16).map(|_| b.next_u32()).collect();
        assert_ne!(sa, sb);
    }

    #[test]
    fn serde_round_trip_continues_sequence() {
        let mut rng = SimRng::new(11, 2);
        rng.next_u64();
        let json = serde_json::to_string(&rng).unwrap();
        let mut restored: SimRng = serde_json::from_str(&json).unwrap();
        for _ in 0..50 {
            assert_eq!(rng.next_u32(), restored.next_u32());
        }
    }

    #[test]
    fn range_rejects_empty_and_invalid() {
        let mut rng = SimRng::new(1, 1);
        assert_eq!(
            rng.range_u32(5, 5),
            Err(CoreError::EmptyRange { lo: 5, hi: 5 })
        );
        assert!(rng.range_u32(6, 5).is_err());
        assert!(rng.range_f32(1.0, 1.0).is_err());
        assert!(rng.range_f32(f32::NAN, 1.0).is_err());
        assert!(rng.range_f32(0.0, f32::INFINITY).is_err());
    }

    proptest! {
        #[test]
        fn next_f32_in_unit_interval(seed in any::<u64>(), stream in any::<u64>()) {
            let mut rng = SimRng::new(seed, stream);
            for _ in 0..64 {
                let v = rng.next_f32();
                prop_assert!((0.0..1.0).contains(&v));
            }
        }

        #[test]
        fn range_u32_stays_in_bounds(
            seed in any::<u64>(),
            lo in 0u32..1_000_000,
            span in 1u32..1_000_000,
        ) {
            let mut rng = SimRng::new(seed, 0);
            let hi = lo + span;
            for _ in 0..64 {
                let v = rng.range_u32(lo, hi).unwrap();
                prop_assert!(v >= lo && v < hi);
            }
        }

        #[test]
        fn range_f32_stays_in_half_open_bounds(
            seed in any::<u64>(),
            lo in -1.0e6f32..1.0e6,
            span in 1.0e-3f32..1.0e6,
        ) {
            let mut rng = SimRng::new(seed, 0);
            let hi = lo + span;
            for _ in 0..64 {
                let v = rng.range_f32(lo, hi).unwrap();
                prop_assert!(v >= lo && v < hi);
            }
        }
    }
}
