//! Sent-packet history for one client. Each packet records what it carried so that an
//! acknowledgement can advance the shadow, drop acknowledged despawns and retire reliable
//! events. Invariant: a slot is only reused after its packet is acknowledged or overwritten.

use engine_core::Tick;
use engine_world::NetId;

use crate::snapshot::{MAX_RELIABLE_EVENTS_PER_PACKET, PacketAck, PacketSequence};

use super::state::{SnapshotWriter, slot_of};

#[derive(Clone)]
pub(super) struct SentPacket {
    pub(super) active: bool,
    pub(super) sequence: PacketSequence,
    pub(super) components: Vec<(NetId, u32, Tick)>,
    pub(super) despawns: Vec<NetId>,
    pub(super) reliable_events: Vec<u32>,
}

impl SentPacket {
    pub(super) fn with_capacity(entities: usize, despawns: usize) -> Self {
        Self {
            active: false,
            sequence: PacketSequence::ZERO,
            components: Vec::with_capacity(entities.saturating_mul(4)),
            despawns: Vec::with_capacity(despawns),
            reliable_events: Vec::with_capacity(MAX_RELIABLE_EVENTS_PER_PACKET),
        }
    }
}

impl SnapshotWriter {
    /// Applies the client's newest packet acknowledgement.
    pub fn acknowledge(&mut self, ack: PacketAck) {
        for packet in &mut self.sent {
            if !packet.active || !ack.acknowledges(packet.sequence) {
                continue;
            }
            for &(entity, component, version) in &packet.components {
                if let Some(Some(tracked)) = self.slots.get_mut(slot_of(entity))
                    && tracked.id == entity
                {
                    tracked.acknowledge(component, version);
                }
            }
            for id in &packet.despawns {
                self.pending_despawns.retain(|pending| pending != id);
            }
            for sequence in &packet.reliable_events {
                let mut index = 0;
                while index < self.reliable_events.len() {
                    if self
                        .reliable_events
                        .get(index)
                        .is_some_and(|event| event.sequence == *sequence)
                    {
                        if let Some(mut event) = self.reliable_events.remove(index) {
                            event.bytes.clear();
                            self.reliable_event_spare.push(event);
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            packet.active = false;
            packet.components.clear();
            packet.despawns.clear();
            packet.reliable_events.clear();
        }
    }
}
