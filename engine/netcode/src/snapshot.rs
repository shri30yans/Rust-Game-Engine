//! Replication state schema and public snapshot APIs.
//!
//! Wire values stay independent per component; the submodules separate packet
//! sequence tracking, event delivery, codecs, capture/application, and writing.

mod apply;
mod encode;
mod events;
mod sequence;
mod writer;

pub use apply::{apply_entity, capture_world, capture_world_into};
pub use encode::{
    DEFAULT_CHARACTER_MOVEMENT_PRECISION_M, NetCodecRegistry, NetEncode, NetOwner, TransformAxis,
    TransformBounds, decode_transform, encode_transform, register_net_owner,
};
pub use events::{NetEvent, ReliableEventReceiver, SequencedEvent, UnreliableEvent};
pub use sequence::{PacketAck, PacketSequence};
pub use writer::{EntityReplication, SnapshotWriter, encoded_size};

use engine_core::Tick;
use engine_world::NetId;
use serde::{Deserialize, Serialize};

/// Number of queued packet sequence values retained by clients and servers.
pub const SNAPSHOT_HISTORY: usize = 64;
/// Entity states that have not changed receive this fraction of their relevance
/// weight, ensuring resting entities are refreshed without dominating packets.
pub const UNCHANGED_PRIORITY_FACTOR: f32 = 0.1;
/// Number of bits per stored quaternion component in smallest-three encoding.
pub const TRANSFORM_ROTATION_BITS: u32 = 10;
/// Maximum reliable events repeated in a single packet.
pub const MAX_RELIABLE_EVENTS_PER_PACKET: usize = 16;
/// Maximum serialized payload reserved for reliable events in one packet.
pub const MAX_RELIABLE_EVENT_BYTES_PER_PACKET: usize = 512;

/// One independently decodable component value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentState {
    /// Stable id assigned by the world's component registry.
    pub component: u32,
    /// Monotonic server revision of this component value.
    pub version: Tick,
    /// Whether this component is sent only on initial replication.
    pub initial_only: bool,
    /// True when this entry removes the component from the entity.
    pub removed: bool,
    /// Complete encoded value for this component.
    pub bytes: Vec<u8>,
}

/// Complete replicated state for one network entity at `tick`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    /// Stable entity identity from the authoritative world.
    pub id: NetId,
    /// Components included for this recipient, in registry id order.
    pub components: Vec<ComponentState>,
}

impl EntityState {
    /// Creates a complete entity state from encoded components.
    #[must_use]
    pub fn new(id: NetId, components: Vec<ComponentState>) -> Self {
        Self { id, components }
    }

    /// Creates reusable storage for the expected component count.
    #[must_use]
    pub fn with_capacity(id: NetId, components: usize) -> Self {
        Self {
            id,
            components: Vec::with_capacity(components),
        }
    }

    /// Reserves encoded storage for each current component value.
    pub fn reserve_component_bytes(&mut self, max_component_bytes: usize) {
        for component in &mut self.components {
            component
                .bytes
                .reserve(max_component_bytes.saturating_sub(component.bytes.len()));
        }
    }
}

impl PartialEq for EntityState {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.components == other.components
    }
}
impl Eq for EntityState {}

/// A capped packet containing complete states for a priority-chosen subset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPacket {
    /// Packet identity used for client acknowledgement.
    pub sequence: PacketSequence,
    /// Authoritative simulation tick represented by these states.
    pub tick: Tick,
    /// World generation this packet belongs to.
    pub travel_epoch: u32,
    /// Complete states selected for this datagram.
    pub entities: Vec<EntityState>,
    /// Pending close entries repeated until an acknowledging packet arrives.
    pub despawns: Vec<NetId>,
    /// Reliable ordered game events repeated until acknowledged.
    pub reliable_events: Vec<SequencedEvent>,
    /// Best-effort events included once when packet space permits.
    pub unreliable_events: Vec<UnreliableEvent>,
    #[serde(skip)]
    spare_entities: Vec<EntityState>,
    #[serde(skip)]
    spare_components: Vec<ComponentState>,
    #[serde(skip)]
    spare_reliable_events: Vec<SequencedEvent>,
    #[serde(skip)]
    spare_unreliable_events: Vec<UnreliableEvent>,
}

impl SnapshotPacket {
    /// Creates an empty packet with storage reserved for the expected workload.
    #[must_use]
    pub fn with_capacity(entities: usize, despawns: usize, events: usize) -> Self {
        Self {
            sequence: PacketSequence::ZERO,
            tick: Tick::ZERO,
            travel_epoch: 0,
            entities: Vec::with_capacity(entities),
            despawns: Vec::with_capacity(despawns),
            reliable_events: Vec::with_capacity(events),
            unreliable_events: Vec::with_capacity(events),
            spare_entities: Vec::with_capacity(entities),
            spare_components: Vec::with_capacity(entities.saturating_mul(4)),
            spare_reliable_events: Vec::with_capacity(events),
            spare_unreliable_events: Vec::with_capacity(events),
        }
    }

    /// Sets the world generation for a packet before encoding it.
    pub fn set_travel_epoch(&mut self, travel_epoch: u32) {
        self.travel_epoch = travel_epoch;
    }

    /// Reserves reusable entity and component buffers from a representative state set.
    pub fn reserve_for(&mut self, states: &[EntityState], max_component_bytes: usize) {
        for state in states {
            let entity = EntityState::with_capacity(state.id, state.components.len());
            for component in &state.components {
                let mut reusable = component.clone();
                reusable
                    .bytes
                    .reserve(max_component_bytes.saturating_sub(reusable.bytes.len()));
                self.spare_components.push(reusable);
            }
            self.spare_entities.push(entity);
        }
    }

    pub(super) fn clear_for_reuse(&mut self, sequence: PacketSequence, tick: Tick) {
        for mut entity in self.entities.drain(..) {
            self.spare_components.append(&mut entity.components);
            self.spare_entities.push(entity);
        }
        self.despawns.clear();
        self.travel_epoch = 0;
        self.spare_reliable_events.append(&mut self.reliable_events);
        self.spare_unreliable_events
            .append(&mut self.unreliable_events);
        self.sequence = sequence;
        self.tick = tick;
    }

    pub(super) fn take_entity(&mut self, id: NetId) -> EntityState {
        let mut entity = self
            .spare_entities
            .pop()
            .unwrap_or_else(|| EntityState::with_capacity(id, 0));
        entity.id = id;
        entity
    }

    pub(super) fn recycle_last_entity(&mut self) {
        if let Some(mut entity) = self.entities.pop() {
            self.spare_components.append(&mut entity.components);
            self.spare_entities.push(entity);
        }
    }

    pub(super) fn take_component(&mut self, component: u32) -> ComponentState {
        let index = self
            .spare_components
            .iter()
            .position(|state| state.component == component);
        if let Some(index) = index {
            self.spare_components.remove(index)
        } else {
            ComponentState {
                component,
                version: Tick::ZERO,
                initial_only: false,
                removed: false,
                bytes: Vec::new(),
            }
        }
    }

    pub(super) fn take_reliable_event(&mut self, sequence: u32, bytes: &[u8]) {
        let mut event = self.spare_reliable_events.pop().unwrap_or(SequencedEvent {
            sequence,
            bytes: Vec::new(),
        });
        event.sequence = sequence;
        event.bytes.clear();
        event.bytes.extend_from_slice(bytes);
        self.reliable_events.push(event);
    }

    pub(super) fn take_unreliable_event(&mut self, bytes: &[u8]) {
        let mut event = self
            .spare_unreliable_events
            .pop()
            .unwrap_or(UnreliableEvent { bytes: Vec::new() });
        event.bytes.clear();
        event.bytes.extend_from_slice(bytes);
        self.unreliable_events.push(event);
    }

    pub(super) fn recycle_last_reliable_event(&mut self) {
        if let Some(event) = self.reliable_events.pop() {
            self.spare_reliable_events.push(event);
        }
    }

    pub(super) fn recycle_last_unreliable_event(&mut self) {
        if let Some(event) = self.unreliable_events.pop() {
            self.spare_unreliable_events.push(event);
        }
    }
}

impl PartialEq for SnapshotPacket {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
            && self.tick == other.tick
            && self.travel_epoch == other.travel_epoch
            && self.entities == other.entities
            && self.despawns == other.despawns
            && self.reliable_events == other.reliable_events
            && self.unreliable_events == other.unreliable_events
    }
}
impl Eq for SnapshotPacket {}
