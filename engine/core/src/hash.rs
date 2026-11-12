//! Platform-stable 64-bit state hasher (FNV-1a).
//!
//! Used for per-tick state hashes that must match across machines. Every
//! value is written as explicit little-endian bytes in the order the caller
//! calls the methods, so the digest depends only on the sequence of writes,
//! never on host endianness, pointer width or struct layout.
//!
//! Field-order rule: callers hash fields in a documented, fixed order. Variable
//! length data goes through [`StateHasher::write_bytes`], which prefixes the
//! length, so adjacent fields cannot run together and collide.
//! Floats hash their bit pattern after canonicalisation: `-0.0` hashes as
//! `+0.0` and every NaN hashes as the canonical quiet NaN.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a 64-bit hasher with explicit, portable encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHasher {
    state: u64,
}

impl StateHasher {
    /// Creates a hasher at the FNV offset basis.
    #[must_use]
    pub const fn new() -> Self {
        StateHasher { state: FNV_OFFSET }
    }

    /// Hashes one byte.
    pub fn write_u8(&mut self, value: u8) {
        self.absorb(value);
    }

    /// Hashes a `u32` as four little-endian bytes.
    pub fn write_u32(&mut self, value: u32) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Hashes a `u64` as eight little-endian bytes.
    pub fn write_u64(&mut self, value: u64) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Hashes an `i32` as four little-endian bytes of its two's-complement form.
    pub fn write_i32(&mut self, value: i32) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Hashes an `f32` by its canonicalised IEEE-754 bit pattern: `-0.0`
    /// becomes `+0.0`, and any NaN becomes the canonical quiet NaN.
    pub fn write_f32(&mut self, value: f32) {
        let bits = if value == 0.0 {
            0.0f32.to_bits()
        } else if value.is_nan() {
            f32::NAN.to_bits()
        } else {
            value.to_bits()
        };
        self.write_u32(bits);
    }

    /// Hashes a byte slice, prefixed by its length as a `u64`.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u64(bytes.len() as u64);
        self.write_raw(bytes);
    }

    /// Returns the 64-bit digest of everything written so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.state
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.absorb(b);
        }
    }

    fn absorb(&mut self, byte: u8) {
        self.state ^= u64::from(byte);
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut h = StateHasher::new();
        for &b in bytes {
            h.write_u8(b);
        }
        h.finish()
    }

    #[test]
    fn golden_fnv1a_known_digests() {
        assert_eq!(StateHasher::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash_bytes(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash_bytes(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn golden_mixed_field_sequence() {
        // Expected value computed independently from the documented encoding:
        // u8 1, u32 2, u64 3, i32 -4, f32 1.5, f32 -0.0, bytes b"abc".
        let mut h = StateHasher::new();
        h.write_u8(1);
        h.write_u32(2);
        h.write_u64(3);
        h.write_i32(-4);
        h.write_f32(1.5);
        h.write_f32(-0.0);
        h.write_bytes(b"abc");
        assert_eq!(h.finish(), 0x7d4a_d9f8_1b72_db70);
    }

    #[test]
    fn negative_zero_and_nan_are_canonical() {
        let digest = |v: f32| {
            let mut h = StateHasher::new();
            h.write_f32(v);
            h.finish()
        };
        assert_eq!(digest(-0.0), digest(0.0));
        let other_nan = f32::from_bits(0x7fc0_0001);
        assert_eq!(digest(f32::NAN), digest(other_nan));
        assert_ne!(digest(1.0), digest(-1.0));
    }

    #[test]
    fn length_prefix_separates_adjacent_byte_fields() {
        let mut a = StateHasher::new();
        a.write_bytes(b"ab");
        a.write_bytes(b"c");
        let mut b = StateHasher::new();
        b.write_bytes(b"a");
        b.write_bytes(b"bc");
        assert_ne!(a.finish(), b.finish());
    }

    proptest! {
        #[test]
        fn equal_input_equal_digest(data in proptest::collection::vec(any::<u8>(), 0..256)) {
            let mut a = StateHasher::new();
            let mut b = StateHasher::new();
            a.write_bytes(&data);
            b.write_bytes(&data);
            prop_assert_eq!(a.finish(), b.finish());
        }

        #[test]
        fn single_byte_change_changes_digest(
            data in proptest::collection::vec(any::<u8>(), 1..128),
            idx in any::<prop::sample::Index>(),
            delta in 1u8..=255,
        ) {
            let i = idx.index(data.len());
            let mut changed = data.clone();
            if let Some(byte) = changed.get_mut(i) {
                *byte = byte.wrapping_add(delta);
            }
            let mut a = StateHasher::new();
            let mut b = StateHasher::new();
            a.write_bytes(&data);
            b.write_bytes(&changed);
            prop_assert_ne!(a.finish(), b.finish());
        }
    }
}
