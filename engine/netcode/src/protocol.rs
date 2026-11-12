//! Wire message shapes, session metadata, and size caps.

use serde::{Deserialize, Serialize};

use crate::input::InputBundle;

/// Label for an encoded message kind (used in size-cap errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// [`Message::Hello`].
    Hello,
    /// [`Message::Welcome`].
    Welcome,
    /// [`Message::Reject`].
    Reject,
    /// [`Message::Inputs`].
    Inputs,
    /// A datagram before the message kind is known.
    Datagram,
    /// A server state packet.
    Snapshot,
    /// [`Message::Session`].
    Session,
    /// [`Message::Possess`].
    Possess,
}

impl core::fmt::Display for MessageKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let label = match self {
            MessageKind::Hello => "hello",
            MessageKind::Welcome => "welcome",
            MessageKind::Reject => "reject",
            MessageKind::Inputs => "inputs",
            MessageKind::Datagram => "datagram",
            MessageKind::Snapshot => "snapshot",
            MessageKind::Session => "session",
            MessageKind::Possess => "possess",
        };
        f.write_str(label)
    }
}

/// Engine wire-format version. Bumped when the postcard layout changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    /// The only protocol version this build speaks.
    pub const CURRENT: ProtocolVersion = ProtocolVersion(2);

    /// Wraps an arbitrary protocol number (for tests and future negotiation).
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        ProtocolVersion(raw)
    }

    /// Returns the raw version number.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// Identifies which game binary is connecting (chosen by the game, not the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(u64);

impl GameId {
    /// Wraps a raw game identifier.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        GameId(raw)
    }

    /// Returns the raw id.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Per-game content or rules version (chosen by the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameVersion(u32);

impl GameVersion {
    /// Wraps a raw version number.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        GameVersion(raw)
    }

    /// Returns the raw version.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Game identity carried in the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameProtocol {
    /// Which game.
    pub id: GameId,
    /// Which revision of that game.
    pub version: GameVersion,
}

/// Datagram delivery semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    /// Ordered, lossless delivery (modelled as a reliable stream in loopback).
    Reliable,
    /// Best-effort delivery; may be dropped or reordered.
    Unreliable,
}

/// Slot assigned by the server after a successful handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientIndex(u8);

impl ClientIndex {
    /// Wraps a raw client slot.
    #[must_use]
    pub const fn new(raw: u8) -> Self {
        ClientIndex(raw)
    }

    /// Returns the raw slot index.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Why a server rejected a handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RejectReason {
    /// Engine protocol versions differ.
    VersionMismatch,
    /// Game id or game version differs from the server's session.
    WrongGame,
    /// No free client slots.
    ServerFull,
}

/// Top-level wire message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message<I> {
    /// Client opens a session.
    Hello {
        /// Engine wire version the client expects.
        version: ProtocolVersion,
        /// Game the client is running.
        game: GameProtocol,
        /// Opaque client-chosen nonce (for future dedup).
        client_nonce: u64,
    },
    /// Server accepted the client.
    Welcome {
        /// Engine wire version in use.
        version: ProtocolVersion,
        /// Assigned player slot.
        client_index: ClientIndex,
    },
    /// Server refused the client.
    Reject {
        /// Why the session was refused.
        reason: RejectReason,
    },
    /// Redundant input frames for one simulation step.
    Inputs(InputBundle<I>),
    /// Complete state values for a priority-selected set of entities.
    Snapshot(crate::snapshot::SnapshotPacket),
    /// Opaque session bytes (lobby requests and replies, encoded by the layer above, which
    /// netcode does not depend on). Reliable channel only.
    Session(Vec<u8>),
    /// The server tells a client which replicated entity it controls; sent again after a
    /// resume. Reliable channel only.
    Possess {
        /// The client's pawn.
        pawn: engine_world::NetId,
    },
}

impl<I> Message<I> {
    /// The channel this kind must travel on: inputs and snapshots are unreliable (newer ones
    /// replace lost ones), everything else is reliable. A receiver refuses a message that
    /// arrived on the other channel.
    #[must_use]
    pub const fn channel(&self) -> Channel {
        match self {
            Message::Inputs(_) | Message::Snapshot(_) => Channel::Unreliable,
            Message::Hello { .. }
            | Message::Welcome { .. }
            | Message::Reject { .. }
            | Message::Session(_)
            | Message::Possess { .. } => Channel::Reliable,
        }
    }
}

/// Largest allowed encoded [`Message::Hello`].
pub const MAX_HELLO_BYTES: usize = 256;
/// Largest allowed encoded [`Message::Welcome`].
pub const MAX_WELCOME_BYTES: usize = 64;
/// Largest allowed encoded [`Message::Reject`].
pub const MAX_REJECT_BYTES: usize = 64;
/// Largest allowed encoded [`Message::Inputs`].
pub const MAX_INPUTS_BYTES: usize = 512;
/// Largest allowed encoded [`Message::Session`]: a full 64-seat room view with its waiting list
/// is about 3 KB, so this leaves room for game settings.
pub const MAX_SESSION_BYTES: usize = 16_384;
/// Largest allowed encoded [`Message::Possess`].
pub const MAX_POSSESS_BYTES: usize = 32;
/// Largest message of any kind, checked before parsing; each kind's own cap is checked after.
pub const MAX_MESSAGE_BYTES: usize = MAX_SESSION_BYTES;
/// Largest datagram the engine sends or parses (every unreliable packet, snapshots included).
///
/// It must fit any path the transport accepts: QUIC (RFC 9000) guarantees 1,200-byte UDP payloads,
/// and after its short header, AEAD tag and DATAGRAM frame quinn leaves 1,162 bytes at that size
/// (measured); 1,150 keeps a margin for longer connection ids. The QUIC transport refuses a path
/// that cannot carry this much, so a cap above the guarantee can never silently lose packets.
pub const MAX_DATAGRAM_BYTES: usize = 1150;

/// Returns the byte cap for a decoded [`Message`] variant.
#[must_use]
pub const fn cap_for_message<I>(msg: &Message<I>) -> (MessageKind, usize) {
    match msg {
        Message::Hello { .. } => (MessageKind::Hello, MAX_HELLO_BYTES),
        Message::Welcome { .. } => (MessageKind::Welcome, MAX_WELCOME_BYTES),
        Message::Reject { .. } => (MessageKind::Reject, MAX_REJECT_BYTES),
        Message::Inputs(_) => (MessageKind::Inputs, MAX_INPUTS_BYTES),
        Message::Snapshot(_) => (MessageKind::Snapshot, MAX_DATAGRAM_BYTES),
        Message::Session(_) => (MessageKind::Session, MAX_SESSION_BYTES),
        Message::Possess { .. } => (MessageKind::Possess, MAX_POSSESS_BYTES),
    }
}
