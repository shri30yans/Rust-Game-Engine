//! Behaviour tests for the snapshot writer and its related codecs.

use super::size::SNAPSHOT_PACKET_CAP;
use super::*;
use crate::protocol::MAX_DATAGRAM_BYTES;
use crate::snapshot::*;
use engine_world::{Transform, World};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct OwnerValue(u16);
impl NetEncode for OwnerValue {}

#[derive(Serialize, Deserialize)]
struct DemoEvent(u8);
impl NetEvent for DemoEvent {}

#[test]
fn writer_caps_packets_and_advances_unsent_entities() {
    let mut world = World::new(1).unwrap();
    let id = world.spawn().unwrap();
    let state = EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![7; 1190],
        }],
    };
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    let packet = writer.write(Tick::new(1), &[state], |_| 1.0).unwrap();
    assert!(encoded_size(&packet).unwrap() <= MAX_DATAGRAM_BYTES);
    assert!(packet.entities.is_empty());
}

#[test]
fn despawns_repeat_until_their_packet_is_acknowledged() {
    let mut world = World::new(1).unwrap();
    let id = world.spawn().unwrap();
    let mut writer = SnapshotWriter::default();
    writer.queue_despawn(id);
    let first = writer.write(Tick::new(1), &[], |_| 1.0).unwrap();
    assert_eq!(first.despawns, vec![id]);
    writer.acknowledge(PacketAck::new(first.sequence));
    let next = writer.write(Tick::new(2), &[], |_| 1.0).unwrap();
    assert!(next.despawns.is_empty());
}

#[test]
fn relevancy_close_is_not_requeued_after_ack() {
    let mut world = World::new(3).unwrap();
    let id = world.spawn().unwrap();
    let state = EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![3],
        }],
    };
    let mut writer = SnapshotWriter::default();
    let initial = writer
        .write_with_policy(Tick::new(1), std::slice::from_ref(&state), |_| {
            EntityReplication::always(1.0)
        })
        .unwrap();
    writer.acknowledge(PacketAck::new(initial.sequence));
    let irrelevant = EntityReplication {
        relevant: false,
        ..EntityReplication::always(1.0)
    };
    let close = writer
        .write_with_policy(Tick::new(2), std::slice::from_ref(&state), |_| irrelevant)
        .unwrap();
    assert_eq!(close.despawns, vec![id]);
    writer.acknowledge(PacketAck::new(close.sequence));
    let after_ack = writer
        .write_with_policy(Tick::new(3), &[state], |_| irrelevant)
        .unwrap();
    assert!(after_ack.despawns.is_empty());
}

#[test]
fn lost_component_stays_dirty_and_acknowledged_component_clears() {
    let mut world = World::new(2).unwrap();
    let id = world.spawn().unwrap();
    let state = EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![3],
        }],
    };
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    let first = writer
        .write(Tick::new(1), core::slice::from_ref(&state), |_| 1.0)
        .unwrap();
    let resent = writer
        .write(Tick::new(2), core::slice::from_ref(&state), |_| 1.0)
        .unwrap();
    assert_eq!(resent.entities.len(), 1);
    writer.acknowledge(PacketAck::new(first.sequence));
    let settled = writer.write(Tick::new(3), &[state], |_| 1.0).unwrap();
    assert!(settled.entities.is_empty());
}

#[test]
fn dormancy_skips_unchanged_state_and_wakes_on_change() {
    let mut world = World::new(3).unwrap();
    let id = world.spawn().unwrap();
    let state = EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![4],
        }],
    };
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    let dormant = |_| EntityReplication {
        dormant: true,
        ..EntityReplication::always(1.0)
    };
    let first = writer
        .write_with_policy(Tick::new(1), core::slice::from_ref(&state), dormant)
        .unwrap();
    assert_eq!(first.entities.len(), 1);
    writer.acknowledge(PacketAck::new(first.sequence));
    assert!(
        writer
            .write_with_policy(Tick::new(2), core::slice::from_ref(&state), dormant)
            .unwrap()
            .entities
            .is_empty()
    );
    let changed = EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![5],
        }],
    };
    assert_eq!(
        writer
            .write_with_policy(Tick::new(3), &[changed], dormant)
            .unwrap()
            .entities
            .len(),
        1
    );
}

#[test]
fn maximum_update_interval_limits_packet_frequency() {
    let mut world = World::new(4).unwrap();
    let id = world.spawn().unwrap();
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    let state = |byte| EntityState {
        id,
        components: vec![ComponentState {
            component: 1,
            version: Tick::new(1),
            initial_only: false,
            removed: false,
            bytes: vec![byte],
        }],
    };
    let policy = |_| EntityReplication {
        max_update_interval_ticks: 3,
        ..EntityReplication::always(1.0)
    };
    let first = writer
        .write_with_policy(Tick::new(1), &[state(1)], policy)
        .unwrap();
    writer.acknowledge(PacketAck::new(first.sequence));
    assert!(
        writer
            .write_with_policy(Tick::new(2), &[state(2)], policy)
            .unwrap()
            .entities
            .is_empty()
    );
    assert_eq!(
        writer
            .write_with_policy(Tick::new(4), &[state(2)], policy)
            .unwrap()
            .entities
            .len(),
        1
    );
}

#[test]
fn initial_only_values_stop_after_ack_and_events_use_packet_acknowledgements() {
    let mut world = World::new(6).unwrap();
    let id = world.spawn().unwrap();
    let initial = EntityState {
        id,
        components: vec![ComponentState {
            component: 2,
            version: Tick::new(1),
            initial_only: true,
            removed: false,
            bytes: vec![1],
        }],
    };
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    writer.queue_reliable_event(&DemoEvent(9)).unwrap();
    writer.queue_unreliable_event(&DemoEvent(8)).unwrap();
    let first = writer
        .write(Tick::new(1), core::slice::from_ref(&initial), |_| 1.0)
        .unwrap();
    assert_eq!(first.reliable_events.len(), 1);
    assert_eq!(first.unreliable_events.len(), 1);
    writer.acknowledge(PacketAck::new(first.sequence));
    let changed = EntityState {
        id,
        components: vec![ComponentState {
            component: 2,
            version: Tick::new(1),
            initial_only: true,
            removed: false,
            bytes: vec![2],
        }],
    };
    let next = writer.write(Tick::new(2), &[changed], |_| 1.0).unwrap();
    assert!(next.entities.is_empty());
    assert!(next.reliable_events.is_empty());
    assert!(next.unreliable_events.is_empty());
}

#[test]
fn component_removal_is_sent_and_applied_as_a_close_value() {
    let mut server = World::new(8).unwrap();
    server
        .register::<OwnerValue>(engine_world::Replication::Replicated)
        .unwrap();
    let server_id = server.spawn().unwrap();
    server.insert(server_id, OwnerValue(22)).unwrap();

    let mut client = World::new(9).unwrap();
    client
        .register::<OwnerValue>(engine_world::Replication::Replicated)
        .unwrap();
    let client_id = client.spawn().unwrap();
    let bounds = TransformBounds::new(
        Vec3::splat(-32.0),
        Vec3::splat(32.0),
        crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    )
    .unwrap();
    let codecs = NetCodecRegistry::default();
    let mut writer = SnapshotWriter::with_capacity(1, 0);
    let first_state = capture_world(&server, None, None, bounds, &codecs).unwrap();
    let first = writer.write(Tick::new(1), &first_state, |_| 1.0).unwrap();
    apply_entity(
        &mut client,
        client_id,
        first.entities.first().unwrap(),
        &codecs,
        bounds,
    )
    .unwrap();
    writer.acknowledge(PacketAck::new(first.sequence));

    server.remove::<OwnerValue>(server_id).unwrap();
    let changed_state = capture_world(&server, None, None, bounds, &codecs).unwrap();
    let changed = writer.write(Tick::new(2), &changed_state, |_| 1.0).unwrap();
    apply_entity(
        &mut client,
        client_id,
        changed.entities.first().unwrap(),
        &codecs,
        bounds,
    )
    .unwrap();
    assert!(client.get::<OwnerValue>(client_id).is_err());
}

#[test]
fn bounded_packets_eventually_serve_every_entity() {
    let mut world = World::new(7).unwrap();
    let mut states = Vec::with_capacity(224);
    let mut seen = Vec::with_capacity(224);
    for _ in 0..224 {
        let id = world.spawn().unwrap();
        seen.push(id);
        states.push(EntityState {
            id,
            components: vec![ComponentState {
                component: 1,
                version: Tick::new(1),
                initial_only: false,
                removed: false,
                bytes: vec![6; 24],
            }],
        });
    }
    let mut writer = SnapshotWriter::with_capacity(224, 16);
    let mut sent = Vec::with_capacity(224);
    let mut byte_count = 0_usize;
    for raw_tick in 0..30_u32 {
        let result = writer.write(Tick::new(raw_tick), &states, |_| 1.0);
        assert!(
            result.is_ok(),
            "pending closes: {}",
            writer.pending_despawns.len()
        );
        let Some(packet) = result.ok() else { return };
        let size = encoded_size(&packet).unwrap();
        assert!(size <= MAX_DATAGRAM_BYTES);
        byte_count = byte_count.saturating_add(size);
        for entity in packet.entities {
            if !sent.contains(&entity.id) {
                sent.push(entity.id);
            }
        }
    }
    assert_eq!(sent.len(), seen.len());
    assert!(byte_count <= 40_000);
}

#[test]
fn transform_quantisation_round_trips_within_one_millimetre() {
    let bounds = TransformBounds::new(
        Vec3::splat(-32.0),
        Vec3::splat(32.0),
        crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    )
    .unwrap();
    let transform = Transform::new(
        Vec3::new(1.234, -4.567, 8.9),
        Quat::from_rotation_y(0.8),
        Vec3::ONE,
    )
    .unwrap();
    let mut bytes = Vec::new();
    encode_transform(&transform, bounds, &mut bytes).unwrap();
    let decoded = decode_transform(&bytes, bounds).unwrap();
    assert!(decoded.translation().distance(transform.translation()) < 0.001);
    assert!(decoded.rotation().angle_between(transform.rotation()) < 0.01);
}

proptest::proptest! {
    #[test]
    fn transform_position_round_trips_within_axis_step(
        x in -127.999_f32..127.999,
        y in -127.999_f32..127.999,
        z in -127.999_f32..127.999,
    ) {
        let bounds = TransformBounds::new(
            Vec3::splat(-128.0),
            Vec3::splat(128.0),
            crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
        ).unwrap();
        let transform = Transform::new(Vec3::new(x, y, z), Quat::IDENTITY, Vec3::ONE).unwrap();
        let mut bytes = Vec::new();
        encode_transform(&transform, bounds, &mut bytes).unwrap();
        let decoded = decode_transform(&bytes, bounds).unwrap();
        let error = decoded.translation() - transform.translation();
        let tolerance = bounds.position_step_m() + 0.000_01;
        proptest::prop_assert!(error.abs().max_element() as f64 <= tolerance);
        proptest::prop_assert!(bounds.position_step_m() <= 0.001);
    }
}

#[test]
fn transform_bounds_reject_precision_that_exceeds_24_bits() {
    let error = TransformBounds::new(
        Vec3::splat(-10_000.0),
        Vec3::splat(10_000.0),
        crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    );
    assert!(matches!(
        error,
        Err(crate::NetError::TransformPrecisionImpossible {
            axis: crate::snapshot::TransformAxis::X
        })
    ));
}

#[test]
fn transform_quantisation_rejects_positions_outside_the_game_bound() {
    let bounds = TransformBounds::new(
        Vec3::splat(-1.0),
        Vec3::splat(1.0),
        crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    )
    .unwrap();
    let transform = Transform::new(Vec3::new(2.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE).unwrap();
    assert!(encode_transform(&transform, bounds, &mut Vec::new()).is_err());
}

#[test]
fn capture_filters_owner_only_components_and_quantises_transform() {
    let mut world = World::new(5).unwrap();
    let owner_id = register_net_owner(&mut world).unwrap();
    let value_id = world
        .register::<OwnerValue>(engine_world::Replication::OwnerOnly)
        .unwrap();
    let id = world.spawn().unwrap();
    world
        .insert(
            id,
            NetOwner {
                client: crate::protocol::ClientIndex::new(2),
            },
        )
        .unwrap();
    world.insert(id, OwnerValue(17)).unwrap();
    let bounds = TransformBounds::new(
        Vec3::splat(-32.0),
        Vec3::splat(32.0),
        crate::snapshot::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    )
    .unwrap();
    let mut codecs = NetCodecRegistry::default();
    codecs.register::<OwnerValue>(&world).unwrap();
    let owner = capture_world(
        &world,
        Some(crate::protocol::ClientIndex::new(2)),
        Some(owner_id),
        bounds,
        &codecs,
    )
    .unwrap();
    let other = capture_world(
        &world,
        Some(crate::protocol::ClientIndex::new(3)),
        Some(owner_id),
        bounds,
        &codecs,
    )
    .unwrap();
    let owner_state = owner.first().unwrap();
    let other_state = other.first().unwrap();
    assert!(
        owner_state
            .components
            .iter()
            .any(|component| component.component == value_id.raw())
    );
    assert!(
        !other_state
            .components
            .iter()
            .any(|component| component.component == value_id.raw())
    );
    let transform_id = world.registry().id_of::<Transform>().unwrap().raw();
    let quantised = owner_state
        .components
        .iter()
        .find(|component| component.component == transform_id)
        .unwrap();
    assert!(decode_transform(&quantised.bytes, bounds).is_ok());
}

#[test]
fn reliable_events_wait_for_gaps_and_deliver_once() {
    let mut receiver = ReliableEventReceiver::with_capacity(8);
    let mut out = Vec::new();
    let later = SequencedEvent {
        sequence: 1,
        bytes: vec![2],
    };
    assert_eq!(receiver.receive(core::slice::from_ref(&later), &mut out), 0);
    assert!(out.is_empty());
    let first = SequencedEvent {
        sequence: 0,
        bytes: vec![1],
    };
    let delivered = receiver.receive(&[first, later.clone()], &mut out);
    assert_eq!(delivered, 2);
    assert_eq!(out.first(), Some(&vec![1]));
    assert_eq!(out.get(1), Some(&vec![2]));
    assert_eq!(receiver.receive(core::slice::from_ref(&later), &mut out), 0);
    assert_eq!(out.first(), Some(&Vec::new()));
}

fn sized_states(world: &mut World, sizes: &[usize]) -> Vec<EntityState> {
    sizes
        .iter()
        .map(|&size| EntityState {
            id: world.spawn().unwrap(),
            components: vec![ComponentState {
                component: 1,
                version: Tick::new(1),
                initial_only: false,
                removed: false,
                bytes: vec![3; size],
            }],
        })
        .collect()
}

proptest::proptest! {
    /// The writer's running size matches a full re-encode: it packs exactly the entities a
    /// greedy fill checked by encoding the whole packet would pack, and never exceeds the cap.
    #[test]
    fn tracked_packet_size_matches_full_encoding(
        sizes in proptest::collection::vec(1_usize..400, 1..160),
    ) {
        let mut world = World::new(3).unwrap();
        let states = sized_states(&mut world, &sizes);
        let mut writer = SnapshotWriter::with_capacity(states.len(), 0);
        let packet = writer.write(Tick::new(1), &states, |_| 1.0).unwrap();
        proptest::prop_assert!(encoded_size(&packet).unwrap() <= SNAPSHOT_PACKET_CAP);

        let mut reference = SnapshotPacket::with_capacity(states.len(), 0, 0);
        reference.sequence = packet.sequence;
        reference.tick = packet.tick;
        let mut ordered: Vec<&EntityState> = states.iter().collect();
        ordered.sort_by_key(|state| state.id);
        for state in ordered {
            reference.entities.push(state.clone());
            if encoded_size(&reference).unwrap() > SNAPSHOT_PACKET_CAP {
                reference.entities.pop();
            }
        }
        let written: Vec<NetId> = packet.entities.iter().map(|entity| entity.id).collect();
        let expected: Vec<NetId> = reference.entities.iter().map(|entity| entity.id).collect();
        proptest::prop_assert_eq!(written, expected);
    }
}

/// Timed, printed, not asserted: one server tick of writes for 24 clients over 224 entities that
/// all change every tick (the worst case; dormancy makes real scenes cheaper).
#[test]
fn server_write_time_for_twenty_four_clients() {
    const CLIENTS: usize = 24;
    const WRITES: u32 = 200;
    let mut world = World::new(5).unwrap();
    let mut states = sized_states(&mut world, &[24; 224]);
    let mut writers: Vec<SnapshotWriter> = (0..CLIENTS)
        .map(|_| SnapshotWriter::with_capacity(states.len(), 16))
        .collect();
    let mut packets: Vec<SnapshotPacket> = (0..CLIENTS)
        .map(|_| SnapshotPacket::with_capacity(states.len(), 16, 16))
        .collect();
    let mut run = |raw_tick: u32, states: &mut [EntityState]| {
        for state in states.iter_mut() {
            if let Some(component) = state.components.first_mut()
                && let Some(byte) = component.bytes.first_mut()
            {
                *byte = byte.wrapping_add(1);
            }
        }
        for (writer, packet) in writers.iter_mut().zip(&mut packets) {
            writer
                .write_into(Tick::new(raw_tick), states, packet, |_| 1.0)
                .unwrap();
            writer.acknowledge(PacketAck::new(packet.sequence));
        }
    };
    for raw_tick in 1..=20 {
        run(raw_tick, &mut states);
    }
    let start = std::time::Instant::now();
    for raw_tick in 21..21 + WRITES {
        run(raw_tick, &mut states);
    }
    let per_tick = start.elapsed() / WRITES;
    println!("server write: {per_tick:?} per tick for {CLIENTS} clients x 224 entities");
}
