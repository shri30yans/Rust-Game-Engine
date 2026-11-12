//! Engine netcode: transport abstraction, deterministic loopback, tick-stamped
//! redundant inputs, and a versioned size-capped wire codec.
//!
//! Invariants: no wall clock, no OS entropy; simulation time advances only
//! through [`engine_core::Tick`]; every decode is bounded by
//! [`protocol::MAX_DATAGRAM_BYTES`] before parsing begins.
#![forbid(unsafe_code)]

pub mod client;
pub mod codec;
pub mod error;
pub mod handshake;
pub mod input;
pub mod loopback;
pub mod loopback_hub;
pub mod predict;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod quic;
pub mod snapshot;
pub mod transport;

pub use client::{
    InterpolationConfig, PacketAckTracker, SnapshotBuffer, blend_transform, interpolate,
};
pub use codec::{decode, encode, encode_snapshot};
pub use error::NetError;
pub use handshake::{ClientHandshake, ServerHandshake};
pub use input::{
    INPUT_HISTORY, INPUT_REDUNDANCY, InputBundle, InputFrame, InputFrames, InputHistory,
    InputPayload, InputReceiver, InputRepeater,
};
pub use loopback::{LinkConditions, LoopbackEnd, loopback_pair};
pub use loopback_hub::LoopbackHub;
pub use predict::{Predicted, PredictionConfig, PredictionHistory};
pub use protocol::{
    Channel, ClientIndex, GameId, GameProtocol, GameVersion, MAX_DATAGRAM_BYTES, MAX_HELLO_BYTES,
    MAX_INPUTS_BYTES, MAX_MESSAGE_BYTES, MAX_POSSESS_BYTES, MAX_REJECT_BYTES, MAX_SESSION_BYTES,
    MAX_WELCOME_BYTES, Message, MessageKind, ProtocolVersion, RejectReason,
};
pub use snapshot::{
    ComponentState, DEFAULT_CHARACTER_MOVEMENT_PRECISION_M, EntityReplication, EntityState,
    MAX_RELIABLE_EVENTS_PER_PACKET, NetCodecRegistry, NetEncode, NetEvent, NetOwner, PacketAck,
    PacketSequence, ReliableEventReceiver, SNAPSHOT_HISTORY, SequencedEvent, SnapshotPacket,
    SnapshotWriter, TransformAxis, TransformBounds, UnreliableEvent, apply_entity, capture_world,
    capture_world_into, decode_transform, encode_transform, register_net_owner,
};
pub use transport::{ClientId, CloseReason, ServerEvent, ServerTransport, Transport};
