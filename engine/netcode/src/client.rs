//! Client-side packet acknowledgement, per-entity timelines, and interpolation.
//!
//! Timelines retain only the newest fixed number of states for each entity. A
//! delayed or reordered state never replaces a newer state.

use engine_core::Tick;
use engine_world::{NetId, Transform, WorldError};
use glam::{Quat, Vec3};

use crate::snapshot::{ComponentState, EntityState, PacketAck, PacketSequence, SnapshotPacket};

/// One entity's received state at a server tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedEntityState {
    /// Server tick represented by this complete state.
    pub tick: Tick,
    /// Complete state for one entity.
    pub state: EntityState,
}

/// Recent complete states for one entity.
#[derive(Debug, Clone)]
pub struct EntityTimeline {
    id: NetId,
    states: Vec<TimedEntityState>,
    spare_states: Vec<TimedEntityState>,
    capacity: usize,
}

impl EntityTimeline {
    fn new(id: NetId, capacity: usize) -> Self {
        Self {
            id,
            states: Vec::with_capacity(capacity),
            spare_states: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Entity identity held by this timeline.
    #[must_use]
    pub const fn id(&self) -> NetId {
        self.id
    }

    /// States from oldest to newest.
    #[must_use]
    pub fn states(&self) -> &[TimedEntityState] {
        &self.states
    }

    fn reserve_from(&mut self, source: &EntityState, max_component_bytes: usize) {
        while self.states.len() + self.spare_states.len() < self.capacity {
            let mut state = source.clone();
            for component in &mut state.components {
                component
                    .bytes
                    .reserve(max_component_bytes.saturating_sub(component.bytes.len()));
                component.version = Tick::ZERO;
            }
            self.spare_states.push(TimedEntityState {
                tick: Tick::ZERO,
                state,
            });
        }
    }

    fn insert(&mut self, tick: Tick, state: &EntityState) {
        if self
            .states
            .last()
            .is_some_and(|last| !tick.is_after(last.tick))
        {
            return;
        }
        if self.states.len() == self.capacity {
            self.states.rotate_left(1);
        }
        if self.states.len() < self.capacity {
            let prepared = self.spare_states.pop().unwrap_or_else(|| TimedEntityState {
                tick,
                state: EntityState::with_capacity(self.id, state.components.len()),
            });
            self.states.push(prepared);
        }
        let Some((target, previous_states)) = self.states.split_last_mut() else {
            return;
        };
        target.tick = tick;
        if let Some(previous) = previous_states.last() {
            copy_state(&mut target.state, &previous.state);
            merge_components(&mut target.state, state);
        } else {
            copy_state(&mut target.state, state);
        }
        target
            .state
            .components
            .sort_by_key(|component| component.component);
    }
}

fn copy_state(target: &mut EntityState, source: &EntityState) {
    target.id = source.id;
    for source_component in &source.components {
        if let Some(component) = target
            .components
            .iter_mut()
            .find(|component| component.component == source_component.component)
        {
            copy_component(component, source_component);
        } else {
            target.components.push(source_component.clone());
        }
    }
}

fn merge_components(target: &mut EntityState, incoming: &EntityState) {
    for incoming_component in &incoming.components {
        if let Some(component) = target
            .components
            .iter_mut()
            .find(|component| component.component == incoming_component.component)
        {
            if incoming_component.version.is_after(component.version) {
                copy_component(component, incoming_component);
            }
        } else {
            target.components.push(incoming_component.clone());
        }
    }
}

fn copy_component(target: &mut ComponentState, source: &ComponentState) {
    target.component = source.component;
    target.version = source.version;
    target.initial_only = source.initial_only;
    target.removed = source.removed;
    target.bytes.clone_from(&source.bytes);
}

/// Fixed-capacity timelines for entities observed by a client.
pub struct SnapshotBuffer {
    entities: Vec<EntityTimeline>,
    entity_capacity: usize,
    states_per_entity: usize,
    ack: PacketAckTracker,
}

impl SnapshotBuffer {
    /// Pre-allocates storage for the configured entity and per-entity history counts.
    #[must_use]
    pub fn with_capacity(entity_capacity: usize, states_per_entity: usize) -> Self {
        Self {
            entities: Vec::with_capacity(entity_capacity),
            entity_capacity,
            states_per_entity: states_per_entity.max(1),
            ack: PacketAckTracker::default(),
        }
    }

    /// Prepares timelines and component byte buffers from representative complete states.
    pub fn reserve_for(&mut self, states: &[EntityState], max_component_bytes: usize) {
        for state in states {
            let timeline_index = self.entities.iter().position(|item| item.id == state.id);
            if let Some(index) = timeline_index {
                if let Some(timeline) = self.entities.get_mut(index) {
                    timeline.reserve_from(state, max_component_bytes);
                }
            } else if self.entities.len() < self.entity_capacity {
                let mut timeline = EntityTimeline::new(state.id, self.states_per_entity);
                timeline.reserve_from(state, max_component_bytes);
                self.entities.push(timeline);
            }
        }
    }

    /// Inserts every entity state in `packet`; stale states are ignored.
    pub fn insert(&mut self, packet: &SnapshotPacket) {
        self.ack.record(packet.sequence);
        self.entities
            .retain(|timeline| !packet.despawns.contains(&timeline.id));
        for state in &packet.entities {
            if let Some(timeline) = self
                .entities
                .iter_mut()
                .find(|timeline| timeline.id == state.id)
            {
                timeline.insert(packet.tick, state);
            } else if self.entities.len() < self.entity_capacity {
                let mut timeline = EntityTimeline::new(state.id, self.states_per_entity);
                timeline.insert(packet.tick, state);
                self.entities.push(timeline);
            }
        }
    }

    /// Timeline for `id`, when the entity is inside the configured client capacity.
    #[must_use]
    pub fn timeline(&self, id: NetId) -> Option<&EntityTimeline> {
        self.entities.iter().find(|timeline| timeline.id == id)
    }

    /// Packet acknowledgement to put in each outgoing input bundle.
    #[must_use]
    pub const fn ack(&self) -> PacketAck {
        self.ack.ack()
    }
}

/// Returns the two states surrounding `render_tick`, with an interpolation fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bracket<'a> {
    /// State at or before the requested time.
    pub older: &'a TimedEntityState,
    /// State at or after the requested time. Equal to `older` when holding.
    pub newer: &'a TimedEntityState,
    /// Blend factor from `older` to `newer`.
    pub alpha: f32,
    /// Number of extrapolated ticks beyond the newest state, capped by caller policy.
    pub extrapolated_ticks: u32,
}

/// Remote rendering delay and capped extrapolation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpolationConfig {
    /// Number of simulation ticks between server state packets.
    pub snapshot_interval_ticks: u32,
    /// Number of packet intervals the client renders behind the server.
    pub delay_intervals: u32,
    /// Maximum missing packet intervals to extrapolate.
    pub max_extrapolation_intervals: u32,
}

impl Default for InterpolationConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_ticks: 2,
            delay_intervals: 2,
            max_extrapolation_intervals: 1,
        }
    }
}

impl InterpolationConfig {
    /// Server tick to render after applying the configured interpolation delay.
    #[must_use]
    pub const fn render_tick(self, server_tick: Tick) -> Tick {
        server_tick.advance(
            0_u32.wrapping_sub(
                self.snapshot_interval_ticks
                    .saturating_mul(self.delay_intervals),
            ),
        )
    }

    /// Maximum missing-state extrapolation in simulation ticks.
    #[must_use]
    pub const fn max_extrapolation_ticks(self) -> u32 {
        self.snapshot_interval_ticks
            .saturating_mul(self.max_extrapolation_intervals)
    }
}

/// Blends transform translation and rotation; scale steps at `older` until alpha reaches one.
///
/// # Errors
/// Returns a world transform error if numerical interpolation creates invalid data.
pub fn blend_transform(
    older: &Transform,
    newer: &Transform,
    alpha: f32,
) -> Result<Transform, WorldError> {
    let t = alpha.clamp(0.0, 1.0);
    let translation: Vec3 = older.translation().lerp(newer.translation(), t);
    let rotation: Quat = older.rotation().slerp(newer.rotation(), t);
    let scale = if t < 1.0 {
        older.scale()
    } else {
        newer.scale()
    };
    Transform::new(translation, rotation, scale)
}

/// Brackets a render tick. Beyond the newest sample, extrapolation is reported only
/// up to `max_extrapolation_ticks`; afterward the newest state is held.
#[must_use]
pub fn interpolate<'a>(
    timeline: &'a EntityTimeline,
    render_tick: Tick,
    max_extrapolation_ticks: u32,
) -> Option<Bracket<'a>> {
    let states = timeline.states();
    let first = states.first()?;
    let last = states.last()?;
    for pair in states.windows(2) {
        let (Some(older), Some(newer)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if (render_tick == older.tick || render_tick.is_after(older.tick))
            && (render_tick == newer.tick || render_tick.is_before(newer.tick))
        {
            let span = older.tick.distance(newer.tick).max(1);
            let elapsed = older.tick.distance(render_tick).min(span);
            return Some(Bracket {
                older,
                newer,
                alpha: tick_ratio(elapsed, span),
                extrapolated_ticks: 0,
            });
        }
    }
    if render_tick.is_before(first.tick) {
        return Some(Bracket {
            older: first,
            newer: first,
            alpha: 0.0,
            extrapolated_ticks: 0,
        });
    }
    let elapsed = render_tick.distance(last.tick);
    let extrapolated = if elapsed <= max_extrapolation_ticks {
        elapsed
    } else {
        0
    };
    Some(Bracket {
        older: last,
        newer: last,
        alpha: 0.0,
        extrapolated_ticks: extrapolated,
    })
}

fn tick_ratio(numerator: u32, denominator: u32) -> f32 {
    if denominator == 0 {
        return 0.0;
    }
    let mut numerator = numerator;
    let mut denominator = denominator;
    while denominator > u32::from(u16::MAX) {
        numerator >>= 1;
        denominator >>= 1;
    }
    let numerator = u16::try_from(numerator).unwrap_or(u16::MAX);
    let denominator = u16::try_from(denominator.max(1)).unwrap_or(u16::MAX);
    f32::from(numerator) / f32::from(denominator)
}

/// Tracks newest packet and the previous 32 received packet sequences.
#[derive(Debug, Clone, Copy, Default)]
pub struct PacketAckTracker {
    newest: Option<PacketSequence>,
    previous: u32,
}

impl PacketAckTracker {
    /// Records a newly received sequence, including reordered packets in the history mask.
    pub fn record(&mut self, sequence: PacketSequence) {
        let Some(newest) = self.newest else {
            self.newest = Some(sequence);
            return;
        };
        let forward = sequence.raw().wrapping_sub(newest.raw());
        if forward != 0 && forward < (1_u32 << 31) {
            self.previous = if forward > 32 {
                0
            } else if forward == 32 {
                1_u32 << 31
            } else {
                (self.previous << forward) | (1_u32 << (forward - 1))
            };
            self.newest = Some(sequence);
        } else {
            let age = newest.raw().wrapping_sub(sequence.raw());
            if (1..=32).contains(&age) {
                self.previous |= 1_u32 << (age - 1);
            }
        }
    }

    /// Current newest-plus-32 acknowledgement.
    #[must_use]
    pub const fn ack(&self) -> PacketAck {
        match self.newest {
            Some(sequence) => PacketAck::with_previous(sequence, self.previous),
            None => PacketAck::NONE,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::snapshot::{ComponentState, SnapshotPacket};
    use engine_world::World;

    fn packet(id: NetId, sequence: u32, tick: u32) -> SnapshotPacket {
        let state = EntityState::new(
            id,
            vec![ComponentState {
                component: 0,
                version: Tick::new(tick),
                initial_only: false,
                removed: false,
                bytes: vec![u8::try_from(tick).unwrap_or(0)],
            }],
        );
        let mut packet = SnapshotPacket::with_capacity(1, 0, 0);
        packet.sequence = PacketSequence::new(sequence);
        packet.tick = Tick::new(tick);
        packet.entities.push(state);
        packet
    }

    #[test]
    fn timeline_ignores_older_states_and_brackets_samples() {
        let mut world = World::new(1).unwrap();
        let id = world.spawn().unwrap();
        let mut buffer = SnapshotBuffer::with_capacity(2, 4);
        buffer.insert(&packet(id, 1, 10));
        buffer.insert(&packet(id, 3, 12));
        buffer.insert(&packet(id, 2, 11));
        let bracket = interpolate(buffer.timeline(id).unwrap(), Tick::new(11), 1).unwrap();
        assert_eq!(bracket.older.tick, Tick::new(10));
        assert_eq!(bracket.newer.tick, Tick::new(12));
        assert_eq!(bracket.alpha, 0.5);
    }

    #[test]
    fn acknowledgement_tracks_reordered_packets() {
        let mut tracker = PacketAckTracker::default();
        tracker.record(PacketSequence::new(12));
        tracker.record(PacketSequence::new(10));
        let ack = tracker.ack();
        assert!(ack.acknowledges(PacketSequence::new(12)));
        assert!(ack.acknowledges(PacketSequence::new(10)));
        assert!(!ack.acknowledges(PacketSequence::new(11)));
    }

    #[test]
    fn interpolation_caps_extrapolation_then_holds() {
        let mut world = World::new(2).unwrap();
        let id = world.spawn().unwrap();
        let mut buffer = SnapshotBuffer::with_capacity(1, 4);
        buffer.insert(&packet(id, 1, 10));
        buffer.insert(&packet(id, 2, 12));
        let timeline = buffer.timeline(id).unwrap();
        assert_eq!(
            interpolate(timeline, Tick::new(13), 1)
                .unwrap()
                .extrapolated_ticks,
            1
        );
        let held = interpolate(timeline, Tick::new(14), 1).unwrap();
        assert_eq!(held.extrapolated_ticks, 0);
        assert_eq!(held.older.tick, Tick::new(12));
        assert_eq!(held.newer.tick, Tick::new(12));
    }

    #[test]
    fn newer_partial_packet_preserves_unchanged_components() {
        let mut world = World::new(3).unwrap();
        let id = world.spawn().unwrap();
        let mut buffer = SnapshotBuffer::with_capacity(1, 4);
        let mut first = packet(id, 1, 10);
        first
            .entities
            .first_mut()
            .unwrap()
            .components
            .push(ComponentState {
                component: 1,
                version: Tick::new(10),
                initial_only: false,
                removed: false,
                bytes: vec![2],
            });
        buffer.insert(&first);
        let mut second = packet(id, 2, 12);
        second
            .entities
            .first_mut()
            .unwrap()
            .components
            .first_mut()
            .unwrap()
            .component = 2;
        buffer.insert(&second);
        let state = &buffer.timeline(id).unwrap().states().last().unwrap().state;
        assert_eq!(state.components.len(), 3);
        assert!(
            state
                .components
                .iter()
                .any(|component| component.component == 0)
        );
        assert!(
            state
                .components
                .iter()
                .any(|component| component.component == 1)
        );
        assert!(
            state
                .components
                .iter()
                .any(|component| component.component == 2)
        );
    }

    #[test]
    fn default_interpolation_delay_is_two_packet_intervals() {
        let config = InterpolationConfig::default();
        assert_eq!(config.render_tick(Tick::new(20)), Tick::new(16));
        assert_eq!(config.max_extrapolation_ticks(), 2);
    }
}
