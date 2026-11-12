//! Room codes (short, shareable, typed by people) and resume tokens (long, secret, kept by the
//! client to take its seat back after a drop).
//!
//! A room code is 6 characters from a 32-letter alphabet without look-alikes (no I, O, 0, 1),
//! so one `u32` draw gives a whole code. It is an address, not a secret: who may enter is the
//! room's admission rule. A resume token is 32 bytes of OS randomness; it never appears in
//! `Debug` output or in a [`RoomView`](crate::RoomView), and comparison takes the same time
//! for every input.

use std::fmt;

use engine_core::SimRng;
use serde::{Deserialize, Serialize};

use crate::error::SessionError;

/// The characters a room code uses: no I, O, 0 or 1, which people misread.
pub const ROOM_CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Characters in a room code.
pub const ROOM_CODE_LEN: usize = 6;

/// A short code that names one room on one server, such as `K7QF2M`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RoomCode([u8; ROOM_CODE_LEN]);

impl RoomCode {
    /// Draws a code. Five bits of one `u32` pick each character.
    #[must_use]
    pub fn generate(rng: &mut SimRng) -> Self {
        let mut bits = rng.next_u32();
        let mut code = [0_u8; ROOM_CODE_LEN];
        for slot in &mut code {
            let index = usize::try_from(bits & 31).unwrap_or_default();
            *slot = ROOM_CODE_ALPHABET.get(index).copied().unwrap_or(b'A');
            bits >>= 5;
        }
        Self(code)
    }

    /// Reads a code the way a person types it: case, spaces and dashes are ignored.
    ///
    /// # Errors
    /// [`SessionError::InvalidCode`] unless exactly 6 alphabet characters remain.
    pub fn parse(text: &str) -> Result<Self, SessionError> {
        let mut code = [0_u8; ROOM_CODE_LEN];
        let mut len = 0;
        for byte in text.bytes().filter(|byte| !matches!(byte, b' ' | b'-')) {
            let upper = byte.to_ascii_uppercase();
            if !ROOM_CODE_ALPHABET.contains(&upper) {
                return Err(SessionError::InvalidCode);
            }
            let slot = code.get_mut(len).ok_or(SessionError::InvalidCode)?;
            *slot = upper;
            len += 1;
        }
        if len == ROOM_CODE_LEN {
            Ok(Self(code))
        } else {
            Err(SessionError::InvalidCode)
        }
    }

    /// The code as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Every byte is from the ASCII alphabet, so this never fails.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Display for RoomCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for RoomCode {
    type Error = SessionError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<RoomCode> for String {
    fn from(code: RoomCode) -> Self {
        code.as_str().to_owned()
    }
}

/// The secret a client keeps to take its seat back after a disconnect.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ResumeToken([u8; 32]);

impl ResumeToken {
    /// A fresh token from the operating system's secure random source.
    ///
    /// # Errors
    /// [`SessionError::Entropy`] when the OS source fails.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn generate() -> Result<Self, SessionError> {
        use ring::rand::SecureRandom;
        let mut bytes = [0_u8; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| SessionError::Entropy)?;
        Ok(Self(bytes))
    }

    /// A token with known bytes, for tests and for a client restoring a saved token.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw bytes, for the client to save.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl PartialEq for ResumeToken {
    /// Compares every byte whatever the first difference, so timing reveals nothing.
    fn eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
}

impl Eq for ResumeToken {}

impl fmt::Debug for ResumeToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResumeToken(..)")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn codes_parse_leniently_and_reject_look_alikes() {
        let code = RoomCode::parse("k7q-f2m").unwrap();
        assert_eq!(code.as_str(), "K7QF2M");
        assert_eq!(RoomCode::parse(" K7 QF 2M ").unwrap(), code);
        assert_eq!(RoomCode::parse("K7QF2O"), Err(SessionError::InvalidCode));
        assert_eq!(RoomCode::parse("K7QF2"), Err(SessionError::InvalidCode));
        assert_eq!(RoomCode::parse("K7QF2MM"), Err(SessionError::InvalidCode));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tokens_hide_their_bytes_and_differ() {
        let a = ResumeToken::generate().unwrap();
        let b = ResumeToken::generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "ResumeToken(..)");
        assert_eq!(ResumeToken::from_bytes(a.to_bytes()), a);
    }

    proptest! {
        #[test]
        fn generated_codes_round_trip_through_text(seed: u64) {
            let mut rng = SimRng::new(seed, 0);
            let code = RoomCode::generate(&mut rng);
            prop_assert_eq!(RoomCode::parse(&code.to_string()).unwrap(), code);
            prop_assert!(code.as_str().bytes().all(|b| ROOM_CODE_ALPHABET.contains(&b)));
        }
    }
}
