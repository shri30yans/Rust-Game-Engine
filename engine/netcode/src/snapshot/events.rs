//! Per-client reliable ordered and best-effort event queues.

use std::collections::VecDeque;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::NetError;

/// A reliable event tagged with its per-client sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequencedEvent {
    /// Per-client event order number.
    pub sequence: u32,
    /// Complete postcard event bytes.
    pub bytes: Vec<u8>,
}

/// A best-effort event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreliableEvent {
    /// Complete postcard event bytes.
    pub bytes: Vec<u8>,
}

/// Typed game event serialized with postcard by the netcode event channel.
pub trait NetEvent: Serialize + DeserializeOwned {
    /// Encodes this event into caller-owned reusable storage.
    ///
    /// # Errors
    /// Returns a postcard serialization error.
    fn encode_event_into(&self, out: &mut Vec<u8>) -> Result<(), NetError> {
        out.clear();
        let buffer = core::mem::take(out);
        *out = postcard::to_extend(self, buffer).map_err(NetError::Decode)?;
        Ok(())
    }
}

/// Client-side ordered reliable event receiver. Repeated events are emitted once,
/// and later events wait in the bounded queue until their predecessor arrives.
pub struct ReliableEventReceiver {
    next_sequence: u32,
    pending: VecDeque<SequencedEvent>,
    spare: VecDeque<SequencedEvent>,
    capacity: usize,
}

impl ReliableEventReceiver {
    /// Creates a receiver with fixed storage for reordered events.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            next_sequence: 0,
            pending: VecDeque::with_capacity(capacity),
            spare: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Adds events and writes newly deliverable payloads into reused output buffers.
    ///
    /// The first returned `count` entries in `out` are the newly delivered events; remaining
    /// entries are empty and retain their byte capacity for the next call.
    pub fn receive(&mut self, events: &[SequencedEvent], out: &mut Vec<Vec<u8>>) -> usize {
        for bytes in out.iter_mut() {
            bytes.clear();
        }
        let mut delivered = 0;
        for event in events {
            if (event.sequence == self.next_sequence
                || sequence_after(event.sequence, self.next_sequence))
                && !self
                    .pending
                    .iter()
                    .any(|pending| pending.sequence == event.sequence)
                && self.pending.len() < self.capacity
            {
                let mut pending = self.spare.pop_front().unwrap_or(SequencedEvent {
                    sequence: event.sequence,
                    bytes: Vec::new(),
                });
                pending.sequence = event.sequence;
                pending.bytes.clear();
                pending.bytes.extend_from_slice(&event.bytes);
                self.pending.push_back(pending);
            }
        }
        while let Some(index) = self
            .pending
            .iter()
            .position(|event| event.sequence == self.next_sequence)
        {
            if let Some(mut event) = self.pending.remove(index) {
                if delivered == out.len() {
                    out.push(Vec::new());
                }
                if let Some(output) = out.get_mut(delivered) {
                    output.extend_from_slice(&event.bytes);
                }
                event.bytes.clear();
                self.spare.push_back(event);
                self.next_sequence = self.next_sequence.wrapping_add(1);
                delivered += 1;
            }
        }
        delivered
    }
}

fn sequence_after(value: u32, reference: u32) -> bool {
    let distance = value.wrapping_sub(reference);
    distance != 0 && distance < (1_u32 << 31)
}
