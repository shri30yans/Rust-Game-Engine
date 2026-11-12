//! Typed errors for engine-netcode.

use engine_core::Tick;
use thiserror::Error;

use crate::protocol::{GameId, GameVersion, MessageKind, ProtocolVersion};
use crate::snapshot::TransformAxis;

/// Errors produced by engine-netcode.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NetError {
    /// An encoded message exceeds the cap for its kind.
    #[error("{kind} message is {size} bytes (cap {cap})")]
    MessageTooLarge {
        /// Which wire message kind was oversized.
        kind: MessageKind,
        /// Encoded size in bytes.
        size: usize,
        /// Maximum allowed size for that kind.
        cap: usize,
    },
    /// Postcard failed to parse a datagram.
    #[error("wire decode failed: {0}")]
    Decode(postcard::Error),
    /// A world operation needed by snapshot capture or application failed.
    #[error("snapshot world operation failed: {0}")]
    World(String),
    /// A snapshot is structurally invalid.
    #[error("invalid snapshot: {0}")]
    InvalidSnapshot(&'static str),
    /// Transform bounds are invalid or fail the requested wire precision.
    #[error("invalid transform bounds: {0}")]
    InvalidTransformBounds(&'static str),
    /// The required transform position precision exceeds the 24-bit codec limit on one axis.
    #[error("transform precision cannot be met on {axis:?} with the 24-bit limit")]
    TransformPrecisionImpossible {
        /// Axis that cannot meet the requested precision.
        axis: TransformAxis,
    },
    /// Client and server disagree on the engine protocol version.
    #[error("protocol version mismatch: ours {ours:?}, theirs {theirs:?}")]
    VersionMismatch {
        /// This side's [`ProtocolVersion`].
        ours: ProtocolVersion,
        /// The peer's [`ProtocolVersion`].
        theirs: ProtocolVersion,
    },
    /// Client [`GameId`] or [`GameVersion`] does not match the server.
    #[error(
        "wrong game: ours id={ours_id:?} ver={ours_ver:?}, theirs id={theirs_id:?} ver={theirs_ver:?}"
    )]
    WrongGame {
        /// Server game id.
        ours_id: GameId,
        /// Server game version.
        ours_ver: GameVersion,
        /// Client game id.
        theirs_id: GameId,
        /// Client game version.
        theirs_ver: GameVersion,
    },
    /// A single input payload exceeds its game's [`InputPayload::MAX_BYTES`].
    #[error("input payload is {size} bytes (cap {cap})")]
    PayloadTooLarge {
        /// Encoded payload size.
        size: usize,
        /// Maximum allowed payload size.
        cap: usize,
    },
    /// The peer dropped its receive side of the link.
    #[error("transport channel closed")]
    ChannelClosed,
    /// [`LinkConditions`] probabilities or delays are out of range.
    #[error("invalid link conditions: {0}")]
    InvalidLinkConditions(&'static str),
    /// Client input tick did not strictly increase.
    #[error("input tick {got:?} is not after {last:?}")]
    InputTickNotIncreasing {
        /// Last accepted tick in the history.
        last: Tick,
        /// Tick on the frame that was rejected.
        got: Tick,
    },
    /// The QUIC endpoint could not be created or configured (bind, TLS, runtime).
    #[error("quic setup failed: {0}")]
    QuicSetup(String),
    /// A payload does not fit the transport's limit for its channel.
    #[error("{channel:?} payload is {size} bytes (cap {cap})")]
    PayloadOverTransportLimit {
        /// Channel the payload was sent on.
        channel: crate::protocol::Channel,
        /// Payload size in bytes.
        size: usize,
        /// The transport's limit.
        cap: usize,
    },
    /// The queue between the game thread and the network thread is full.
    #[error("network send queue is full")]
    SendQueueFull,
    /// The connection is not open yet (still connecting).
    #[error("not connected yet")]
    NotConnected,
    /// The server already has as many clients as it accepts.
    #[error("the server is full")]
    ServerFull,
}
