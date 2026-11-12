//! Size-capped postcard encode and decode for [`Message`].

use crate::error::NetError;
use crate::input::{InputPayload, encoded_payload_len, validate_bundle};
use crate::protocol::{
    MAX_DATAGRAM_BYTES, MAX_MESSAGE_BYTES, Message, MessageKind, cap_for_message,
};
use crate::snapshot::SnapshotPacket;
use serde::Serialize;

/// Serialises `msg` into `buf` (cleared first) and enforces the per-kind size cap.
///
/// # Errors
/// Returns [`NetError::MessageTooLarge`] or payload errors when limits are exceeded.
pub fn encode<I: InputPayload>(msg: &Message<I>, buf: &mut Vec<u8>) -> Result<(), NetError> {
    if let Message::Snapshot(packet) = msg {
        return encode_snapshot(packet, buf);
    }
    if let Message::Inputs(bundle) = msg {
        for frame in bundle.frames().iter() {
            encoded_payload_len(frame.payload())?;
        }
        validate_bundle(bundle)?;
    }

    let (kind, cap) = cap_for_message(msg);
    encode_capped(msg, buf, kind, cap)
}

/// Encodes a snapshot directly from a borrowed packet into reusable storage.
///
/// # Errors
/// Returns [`NetError::MessageTooLarge`] or a postcard serialization error.
pub fn encode_snapshot(packet: &SnapshotPacket, buf: &mut Vec<u8>) -> Result<(), NetError> {
    encode_capped(
        &SnapshotMessage(packet),
        buf,
        MessageKind::Snapshot,
        MAX_DATAGRAM_BYTES,
    )
}

fn encode_capped<T: Serialize>(
    value: &T,
    buf: &mut Vec<u8>,
    kind: MessageKind,
    cap: usize,
) -> Result<(), NetError> {
    buf.clear();
    let taken = core::mem::take(buf);
    let encoded = postcard::to_extend(value, taken).map_err(NetError::Decode)?;
    if encoded.len() > cap {
        return Err(NetError::MessageTooLarge {
            kind,
            size: encoded.len(),
            cap,
        });
    }
    *buf = encoded;
    Ok(())
}

struct SnapshotMessage<'a>(&'a SnapshotPacket);

impl Serialize for SnapshotMessage<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_newtype_variant("Message", 4, "Snapshot", self.0)
    }
}

/// Parses a message after bounded-length and payload checks: any message is at most
/// [`MAX_MESSAGE_BYTES`] before parsing, and its own kind's cap after.
///
/// # Errors
/// Returns [`NetError`] when the datagram is oversized or invalid.
pub fn decode<I: InputPayload>(bytes: &[u8]) -> Result<Message<I>, NetError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(NetError::MessageTooLarge {
            kind: MessageKind::Datagram,
            size: bytes.len(),
            cap: MAX_MESSAGE_BYTES,
        });
    }
    let msg: Message<I> = postcard::from_bytes(bytes).map_err(NetError::Decode)?;
    if let Message::Inputs(bundle) = &msg {
        validate_bundle(bundle)?;
        for frame in bundle.frames().iter() {
            encoded_payload_len(frame.payload())?;
        }
    }
    let (kind, cap) = cap_for_message(&msg);
    if bytes.len() > cap {
        return Err(NetError::MessageTooLarge {
            kind,
            size: bytes.len(),
            cap,
        });
    }
    Ok(msg)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::snapshot::{PacketSequence, SnapshotPacket};
    use engine_core::Tick;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Probe(u8);

    impl InputPayload for Probe {
        const MAX_BYTES: usize = 1;
        fn validate(&self) -> Result<(), NetError> {
            Ok(())
        }
    }

    #[test]
    fn snapshot_message_round_trips() {
        let mut packet = SnapshotPacket::with_capacity(0, 0, 0);
        packet.sequence = PacketSequence::new(8);
        packet.tick = Tick::new(120);
        packet.set_travel_epoch(7);
        let message = Message::<Probe>::Snapshot(packet);
        let mut bytes = Vec::new();
        encode(&message, &mut bytes).unwrap();
        assert_eq!(decode::<Probe>(&bytes).unwrap(), message);
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_datagrams_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1300)) {
            let _ = decode::<Probe>(&bytes);
        }
    }
}
