//! Encoded-size budgeting for snapshot packets. Sizes come from the wire's own postcard
//! encoder (size flavor, no allocation), so the budget counts exactly the bytes sent.
//! Invariant: a packet is only grown while its encoded size stays within `SNAPSHOT_PACKET_CAP`.

use crate::error::NetError;
use crate::protocol::MAX_DATAGRAM_BYTES;
use crate::snapshot::SnapshotPacket;

/// Postcard enum tag surrounding the packet in `Message::Snapshot`.
const SNAPSHOT_MESSAGE_TAG_BYTES: usize = 1;
/// Packet payload bytes available after reserving the outer message tag.
pub(super) const SNAPSHOT_PACKET_CAP: usize = MAX_DATAGRAM_BYTES - SNAPSHOT_MESSAGE_TAG_BYTES;

/// Encoded byte size of a packet using its canonical postcard layout, computed without allocating.
///
/// # Errors
/// Returns a serialization error for unsupported packet data.
pub fn encoded_size(packet: &SnapshotPacket) -> Result<usize, NetError> {
    item_size(Some(packet))
}

/// Encoded byte size of one item (zero for `None`), computed without allocating.
pub(super) fn item_size<T: serde::Serialize>(item: Option<&T>) -> Result<usize, NetError> {
    item.map_or(Ok(0), |item| {
        postcard::serialize_with_flavor(item, postcard::ser_flavors::Size::default())
            .map_err(NetError::Decode)
    })
}

/// Extra bytes the varint length prefix of a sequence needs when it grows to `new_len` items.
pub(super) fn length_growth(new_len: usize) -> usize {
    varint_len(new_len).saturating_sub(varint_len(new_len.saturating_sub(1)))
}

fn varint_len(value: usize) -> usize {
    let bits = usize::BITS - value.leading_zeros();
    usize::try_from(bits.div_ceil(7).max(1)).unwrap_or(usize::MAX)
}
