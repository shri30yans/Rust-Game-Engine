//! Per-client writer storage and construction. Holds every per-entity record in a slot
//! indexed by `NetId::index`, the sent-packet history and the queued events. The other writer
//! files add the behaviour; this one owns the fields.

use std::collections::VecDeque;

use engine_world::NetId;

use crate::snapshot::{PacketSequence, SNAPSHOT_HISTORY, SequencedEvent, UnreliableEvent};

use super::ack::SentPacket;
use super::tracked::Tracked;

/// Per-client writer with acknowledged component shadow state and bounded packet history.
///
/// Every per-entity record lives in a slot indexed by the entity's [`NetId::index`] and is checked
/// against the full id, so lookups are O(1) and a recycled index starts from an empty shadow. The
/// packet size is tracked exactly as items are added (postcard's size flavor, no allocation), so a
/// write costs O(entities + components) rather than re-encoding the packet per entity.
pub struct SnapshotWriter {
    pub(super) next_sequence: PacketSequence,
    /// Tracked entities by `NetId::index`.
    pub(super) slots: Vec<Option<Tracked>>,
    /// Slot indices of tracked entities, sorted by accumulator each write.
    pub(super) order: Vec<u32>,
    /// Counts writes; a tracked entity not stamped with the current write was not in `states`.
    pub(super) write_count: u64,
    pub(super) pending_despawns: Vec<NetId>,
    pub(super) sent: Vec<SentPacket>,
    pub(super) reliable_events: VecDeque<SequencedEvent>,
    pub(super) reliable_event_spare: Vec<SequencedEvent>,
    pub(super) unreliable_events: VecDeque<UnreliableEvent>,
    pub(super) unreliable_event_spare: Vec<UnreliableEvent>,
    pub(super) next_event_sequence: u32,
    pub(super) max_component_bytes: usize,
}

pub(super) fn slot_of(id: NetId) -> usize {
    usize::try_from(id.index()).unwrap_or(usize::MAX)
}

impl SnapshotWriter {
    /// Creates a writer with space reserved for the expected entity count.
    #[must_use]
    pub fn with_capacity(entities: usize, despawns: usize) -> Self {
        Self {
            next_sequence: PacketSequence::ZERO,
            slots: Vec::with_capacity(entities),
            order: Vec::with_capacity(entities),
            write_count: 0,
            pending_despawns: Vec::with_capacity(despawns),
            sent: (0..SNAPSHOT_HISTORY)
                .map(|_| SentPacket::with_capacity(entities, despawns))
                .collect(),
            reliable_events: VecDeque::with_capacity(32),
            reliable_event_spare: Vec::with_capacity(32),
            unreliable_events: VecDeque::with_capacity(32),
            unreliable_event_spare: Vec::with_capacity(32),
            next_event_sequence: 0,
            max_component_bytes: 0,
        }
    }

    /// Reserves reusable state storage for the largest encoded component expected.
    pub fn reserve_component_bytes(&mut self, max_component_bytes: usize) {
        self.max_component_bytes = max_component_bytes;
        for tracked in self.slots.iter_mut().flatten() {
            tracked.state.reserve_component_bytes(max_component_bytes);
        }
    }
}

impl Default for SnapshotWriter {
    fn default() -> Self {
        Self::with_capacity(64, 16)
    }
}
