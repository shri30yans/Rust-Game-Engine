//! Packet sequence numbers and the newest-plus-history acknowledgement window.

use serde::{Deserialize, Serialize};

/// Monotonic wrapping sequence assigned to a state packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PacketSequence(u32);

impl PacketSequence {
    /// Sequence number zero, used before a packet has been sent.
    pub const ZERO: Self = Self(0);

    /// Creates a sequence from its wire value.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw wire value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Next sequence, wrapping at the end of the counter.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Newest state packet observed by a client, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PacketAck {
    newest: Option<PacketSequence>,
    previous: u32,
}

impl PacketAck {
    /// No state packet has been received yet.
    pub const NONE: Self = Self {
        newest: None,
        previous: 0,
    };

    /// Acknowledgement for `sequence`.
    #[must_use]
    pub const fn new(sequence: PacketSequence) -> Self {
        Self {
            newest: Some(sequence),
            previous: 0,
        }
    }

    /// Acknowledgement with the previous 32 receive bits.
    #[must_use]
    pub const fn with_previous(sequence: PacketSequence, previous: u32) -> Self {
        Self {
            newest: Some(sequence),
            previous,
        }
    }

    /// Newest received sequence, if one exists.
    #[must_use]
    pub const fn sequence(self) -> Option<PacketSequence> {
        self.newest
    }

    /// Previous 32 receive bits; bit zero acknowledges newest minus one.
    #[must_use]
    pub const fn previous(self) -> u32 {
        self.previous
    }

    /// Returns whether `sequence` is represented by this newest-plus-32 ack window.
    #[must_use]
    pub fn acknowledges(self, sequence: PacketSequence) -> bool {
        let Some(newest) = self.newest else {
            return false;
        };
        if sequence == newest {
            return true;
        }
        let age = newest.raw().wrapping_sub(sequence.raw());
        (1..=32).contains(&age) && (self.previous & (1_u32 << (age - 1))) != 0
    }
}
