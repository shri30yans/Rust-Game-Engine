//! Packet assembly: the public write entry points and the budgeted fill of one packet, in
//! priority order (reliable events, despawns, entities, then unreliable events).
//! Invariant: each item is added only if its exact encoded size keeps the packet within the cap.

use engine_core::Tick;
use engine_world::NetId;

use crate::error::NetError;
use crate::snapshot::{
    EntityState, MAX_RELIABLE_EVENT_BYTES_PER_PACKET, MAX_RELIABLE_EVENTS_PER_PACKET,
    SNAPSHOT_HISTORY, SnapshotPacket,
};

use super::policy::EntityReplication;
use super::size::{SNAPSHOT_PACKET_CAP, encoded_size, item_size, length_growth};
use super::state::SnapshotWriter;

impl SnapshotWriter {
    /// Builds one packet from complete states. Higher relevance is sent sooner;
    /// unchanged states accumulate at [`UNCHANGED_PRIORITY_FACTOR`] weight.
    ///
    /// # Errors
    /// Returns [`NetError::MessageTooLarge`] if packet metadata alone exceeds the cap.
    pub fn write(
        &mut self,
        tick: Tick,
        states: &[EntityState],
        mut relevance: impl FnMut(NetId) -> f32,
    ) -> Result<SnapshotPacket, NetError> {
        let mut packet =
            SnapshotPacket::with_capacity(states.len(), self.pending_despawns.len(), 0);
        self.write_into_with_policy(tick, states, &mut packet, |id| {
            EntityReplication::always(relevance(id))
        })?;
        Ok(packet)
    }

    /// Builds one packet in caller-owned reusable storage.
    ///
    /// # Errors
    /// Returns [`NetError::MessageTooLarge`] when required packet metadata exceeds the cap.
    pub fn write_into(
        &mut self,
        tick: Tick,
        states: &[EntityState],
        packet: &mut SnapshotPacket,
        mut relevance: impl FnMut(NetId) -> f32,
    ) -> Result<(), NetError> {
        self.write_into_with_policy(tick, states, packet, |id| {
            EntityReplication::always(relevance(id))
        })
    }

    /// Builds one packet using game-provided relevance, priority, dormancy, and update rate.
    ///
    /// # Errors
    /// Returns [`NetError::MessageTooLarge`] when required packet metadata exceeds the cap.
    pub fn write_with_policy(
        &mut self,
        tick: Tick,
        states: &[EntityState],
        policy_for: impl FnMut(NetId) -> EntityReplication,
    ) -> Result<SnapshotPacket, NetError> {
        let mut packet =
            SnapshotPacket::with_capacity(states.len(), self.pending_despawns.len(), 0);
        self.write_into_with_policy(tick, states, &mut packet, policy_for)?;
        Ok(packet)
    }

    /// Writes in pre-sized packet storage with the complete game policy.
    ///
    /// # Errors
    /// Returns [`NetError::MessageTooLarge`] when required packet metadata exceeds the cap.
    pub fn write_into_with_policy(
        &mut self,
        tick: Tick,
        states: &[EntityState],
        packet: &mut SnapshotPacket,
        mut policy_for: impl FnMut(NetId) -> EntityReplication,
    ) -> Result<(), NetError> {
        self.refresh(tick, states, &mut policy_for);
        let slots = &self.slots;
        self.order.sort_unstable_by(|a, b| {
            let (Some(Some(a)), Some(Some(b))) = (
                slots.get(usize::try_from(*a).unwrap_or(usize::MAX)),
                slots.get(usize::try_from(*b).unwrap_or(usize::MAX)),
            ) else {
                return a.cmp(b);
            };
            b.accumulator
                .total_cmp(&a.accumulator)
                .then_with(|| a.id.cmp(&b.id))
        });
        packet.clear_for_reuse(self.next_sequence, tick);
        let mut size = encoded_size(packet)?;
        let mut reliable_event_bytes = 0_usize;
        for index in 0..self
            .reliable_events
            .len()
            .min(MAX_RELIABLE_EVENTS_PER_PACKET)
        {
            let Some(event) = self.reliable_events.get(index) else {
                continue;
            };
            if reliable_event_bytes.saturating_add(event.bytes.len())
                > MAX_RELIABLE_EVENT_BYTES_PER_PACKET
            {
                break;
            }
            packet.take_reliable_event(event.sequence, &event.bytes);
            let grown = size
                + item_size(packet.reliable_events.last())?
                + length_growth(packet.reliable_events.len());
            if grown > SNAPSHOT_PACKET_CAP {
                packet.recycle_last_reliable_event();
                break;
            }
            size = grown;
            reliable_event_bytes += event.bytes.len();
        }
        for despawn in &self.pending_despawns {
            let grown = size + item_size(Some(despawn))? + length_growth(packet.despawns.len() + 1);
            if grown > SNAPSHOT_PACKET_CAP {
                break;
            }
            packet.despawns.push(*despawn);
            size = grown;
        }
        for &slot in &self.order {
            let Some(Some(tracked)) = self
                .slots
                .get_mut(usize::try_from(slot).unwrap_or(usize::MAX))
            else {
                continue;
            };
            if !tracked.policy.relevant
                || tracked.sleeping
                || tracked.last_sent.is_some_and(|last| {
                    tick.distance(last) < tracked.policy.max_update_interval_ticks
                })
            {
                continue;
            }
            let mut entity = packet.take_entity(tracked.id);
            for component in &tracked.state.components {
                if !tracked.is_dirty(component) {
                    continue;
                }
                let mut value = packet.take_component(component.component);
                value.version = component.version;
                value.initial_only = component.initial_only;
                value.removed = component.removed;
                value.bytes.clear();
                value.bytes.extend_from_slice(&component.bytes);
                entity.components.push(value);
            }
            if entity.components.is_empty() {
                packet.spare_entities.push(entity);
                continue;
            }
            let grown = size + item_size(Some(&entity))? + length_growth(packet.entities.len() + 1);
            packet.entities.push(entity);
            if grown > SNAPSHOT_PACKET_CAP {
                packet.recycle_last_entity();
                continue;
            }
            size = grown;
            tracked.accumulator = 0.0;
            tracked.last_sent = Some(tick);
            tracked.sleeping = tracked.policy.dormant;
        }
        for event in &self.unreliable_events {
            packet.take_unreliable_event(&event.bytes);
            let grown = size
                + item_size(packet.unreliable_events.last())?
                + length_growth(packet.unreliable_events.len());
            if grown > SNAPSHOT_PACKET_CAP {
                packet.recycle_last_unreliable_event();
                break;
            }
            size = grown;
        }
        for _ in 0..packet.unreliable_events.len() {
            if let Some(event) = self.unreliable_events.pop_front() {
                self.unreliable_event_spare.push(event);
            }
        }
        let slot_index = usize::try_from(packet.sequence.raw()).unwrap_or(0) % SNAPSHOT_HISTORY;
        if let Some(slot) = self.sent.get_mut(slot_index) {
            slot.sequence = packet.sequence;
            slot.active = true;
            slot.components.clear();
            slot.despawns.clear();
            slot.reliable_events.clear();
            for entity in &packet.entities {
                for component in &entity.components {
                    slot.components
                        .push((entity.id, component.component, component.version));
                }
            }
            slot.despawns.extend_from_slice(&packet.despawns);
            slot.reliable_events
                .extend(packet.reliable_events.iter().map(|event| event.sequence));
        }
        self.next_sequence = self.next_sequence.next();
        Ok(())
    }
}
