//! Writer queues: despawns and events the game queues for delivery. Reliable items stay
//! until a packet carrying them is acknowledged; unreliable items are written once.

use engine_world::NetId;

use crate::error::NetError;
use crate::snapshot::{NetEvent, SequencedEvent, UnreliableEvent};

use super::state::SnapshotWriter;

impl SnapshotWriter {
    /// Queues a despawn for reliable repetition until acknowledged.
    pub fn queue_despawn(&mut self, id: NetId) {
        if !self.pending_despawns.contains(&id) {
            self.pending_despawns.push(id);
        }
    }

    /// Queues an ordered event for repetition until a packet carrying it is acknowledged.
    ///
    /// # Errors
    /// Returns a serialization error for unsupported event data.
    pub fn queue_reliable_event<E: NetEvent>(&mut self, event: &E) -> Result<(), NetError> {
        let mut slot = self.reliable_event_spare.pop().unwrap_or(SequencedEvent {
            sequence: self.next_event_sequence,
            bytes: Vec::new(),
        });
        slot.sequence = self.next_event_sequence;
        event.encode_event_into(&mut slot.bytes)?;
        self.reliable_events.push_back(slot);
        self.next_event_sequence = self.next_event_sequence.wrapping_add(1);
        Ok(())
    }

    /// Queues a best-effort event for one packet attempt.
    ///
    /// # Errors
    /// Returns a serialization error for unsupported event data.
    pub fn queue_unreliable_event<E: NetEvent>(&mut self, event: &E) -> Result<(), NetError> {
        let mut slot = self
            .unreliable_event_spare
            .pop()
            .unwrap_or(UnreliableEvent { bytes: Vec::new() });
        event.encode_event_into(&mut slot.bytes)?;
        self.unreliable_events.push_back(slot);
        Ok(())
    }
}
