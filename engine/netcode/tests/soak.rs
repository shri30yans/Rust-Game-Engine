//! Lossy-link replication soak and steady-state allocator regression.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use engine_core::Tick;
use engine_netcode::client::SnapshotBuffer;
use engine_netcode::codec::{decode, encode, encode_snapshot};
use engine_netcode::input::{InputFrame, InputHistory, InputPayload};
use engine_netcode::protocol::{Channel, Message};
use engine_netcode::snapshot::{
    EntityReplication, NetCodecRegistry, NetEvent, ReliableEventReceiver, SnapshotPacket,
    SnapshotWriter, TransformBounds, apply_entity, capture_world, capture_world_into,
    register_net_owner,
};
use engine_netcode::{LinkConditions, Transport, loopback_pair};
use engine_world::{Replication, World};
use serde::{Deserialize, Serialize};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PLAYER_COUNT: usize = 24;
const DEBRIS_COUNT: usize = 200;
const ENTITY_COUNT: usize = PLAYER_COUNT + DEBRIS_COUNT;
const WARMUP_TICKS: u32 = 60;
const MEASURED_TICKS: u32 = 600;
const SOAK_TICKS: u32 = 1800;
const DEBRIS_REST_TICK: u32 = 180;
const PLAYER_STOP_TICK: u32 = 1200;
const SNAPSHOT_EVERY_TICKS: u32 = 2;
const EVENT_EVERY_TICKS: u32 = 20;
const ONE_WAY_DELAY_TICKS: u32 = 5;
const JITTER_TICKS: u32 = 2;
const LOSS_PER_MILLE: u16 = 50;
const MAX_BANDWIDTH_BYTES_PER_SECOND: usize = 40_000;
/// Measured result from the previous fixed 16-bit transform position codec.
const PREVIOUS_TRANSFORM_CODEC_BANDWIDTH_BYTES_PER_SECOND: f64 = 9_500.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Position {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct AckInput(u8);

impl InputPayload for AckInput {
    const MAX_BYTES: usize = 1;

    fn validate(&self) -> Result<(), engine_netcode::NetError> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct TestEvent(u32);
impl NetEvent for TestEvent {}

#[test]
fn lossy_scene_stays_replicated_with_bounded_bandwidth_and_no_steady_allocations() {
    let bounds = TransformBounds::new(
        glam::Vec3::splat(-2048.0),
        glam::Vec3::splat(2048.0),
        engine_netcode::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
    )
    .unwrap();
    let mut server_world = make_world(1);
    let mut client_world = make_world(2);
    let mut server_ids = Vec::with_capacity(ENTITY_COUNT);
    let mut client_ids = Vec::with_capacity(ENTITY_COUNT);
    server_world
        .register::<Position>(Replication::Replicated)
        .unwrap();
    client_world
        .register::<Position>(Replication::Replicated)
        .unwrap();
    let owner_id = register_net_owner(&mut server_world).unwrap();
    register_net_owner(&mut client_world).unwrap();
    spawn_scene(&mut server_world, &mut server_ids);
    spawn_scene(&mut client_world, &mut client_ids);
    assert_eq!(server_ids, client_ids);

    let codecs = NetCodecRegistry::default();
    let mut captured = Vec::with_capacity(ENTITY_COUNT);
    let mut capture_scratch = Vec::with_capacity(128);
    let mut writer = SnapshotWriter::with_capacity(ENTITY_COUNT, ENTITY_COUNT);
    writer.reserve_component_bytes(64);
    let mut packet = SnapshotPacket::with_capacity(ENTITY_COUNT, ENTITY_COUNT, 32);
    let mut packet_bytes = Vec::with_capacity(engine_netcode::MAX_DATAGRAM_BYTES);
    let mut receive_bytes = Vec::with_capacity(engine_netcode::MAX_DATAGRAM_BYTES);
    let mut input_bytes = Vec::with_capacity(engine_netcode::MAX_DATAGRAM_BYTES);
    let mut input_history = InputHistory::new();
    let mut replica = SnapshotBuffer::with_capacity(ENTITY_COUNT, 8);
    let mut event_receiver = ReliableEventReceiver::with_capacity(64);
    let mut event_output = Vec::with_capacity(16);
    let mut event_order = Vec::with_capacity((SOAK_TICKS / EVENT_EVERY_TICKS) as usize);
    let conditions =
        LinkConditions::new(ONE_WAY_DELAY_TICKS, JITTER_TICKS, LOSS_PER_MILLE, 0).unwrap();
    let (mut server_link, mut client_link) = loopback_pair(conditions, conditions, 0x5eed);
    capture_world_into(
        &server_world,
        None,
        Some(owner_id),
        bounds,
        &codecs,
        &mut captured,
        &mut capture_scratch,
    )
    .unwrap();
    for state in &mut captured {
        state.reserve_component_bytes(64);
    }
    packet.reserve_for(&captured, 64);
    replica.reserve_for(&captured, 64);

    let mut downstream_bytes = 0_usize;
    let mut player_updates = 0_usize;
    let mut debris_updates = 0_usize;
    let mut server_allocations = 0_usize;
    let mut server_reallocations = 0_usize;
    let mut client_allocations = 0_usize;
    let mut client_reallocations = 0_usize;

    for raw_tick in 0..SOAK_TICKS {
        let tick = Tick::new(raw_tick);
        move_scene(&mut server_world, &server_ids, raw_tick);
        server_link.advance_to(tick);
        client_link.advance_to(tick);

        while let Some(channel) = server_link.recv(&mut receive_bytes).unwrap() {
            assert_eq!(channel, Channel::Unreliable);
            if let Message::Inputs(bundle) = decode::<AckInput>(&receive_bytes).unwrap() {
                writer.acknowledge(bundle.snapshot_ack());
            }
        }
        while let Some(channel) = client_link.recv(&mut receive_bytes).unwrap() {
            assert_eq!(channel, Channel::Unreliable);
            let Message::Snapshot(incoming) = decode::<AckInput>(&receive_bytes).unwrap() else {
                continue;
            };
            let measured = (WARMUP_TICKS..WARMUP_TICKS + MEASURED_TICKS).contains(&raw_tick);
            let region = measured.then(|| Region::new(GLOBAL));
            replica.insert(&incoming);
            for entity in &incoming.entities {
                if let Some(latest) = replica
                    .timeline(entity.id)
                    .and_then(|timeline| timeline.states().last())
                {
                    apply_entity(&mut client_world, entity.id, &latest.state, &codecs, bounds)
                        .unwrap();
                }
            }
            let delivered = event_receiver.receive(&incoming.reliable_events, &mut event_output);
            if let Some(region) = region {
                let stats = region.change();
                client_allocations += stats.allocations;
                client_reallocations += stats.reallocations;
            }
            for bytes in event_output.iter().take(delivered) {
                let TestEvent(sequence) = postcard::from_bytes(bytes).unwrap();
                event_order.push(sequence);
            }
        }

        input_history
            .push(InputFrame::new(tick, AckInput(0)).unwrap())
            .unwrap();
        input_history.acknowledge_snapshots(replica.ack());
        if let Some(bundle) = input_history.bundle() {
            encode(&Message::Inputs(bundle), &mut input_bytes).unwrap();
            client_link.send(Channel::Unreliable, &input_bytes).unwrap();
        }

        if raw_tick % SNAPSHOT_EVERY_TICKS == 0 {
            let measured = (WARMUP_TICKS..WARMUP_TICKS + MEASURED_TICKS).contains(&raw_tick);
            let region = measured.then(|| Region::new(GLOBAL));
            capture_world_into(
                &server_world,
                None,
                Some(owner_id),
                bounds,
                &codecs,
                &mut captured,
                &mut capture_scratch,
            )
            .unwrap();
            if raw_tick % EVENT_EVERY_TICKS == 0 {
                writer
                    .queue_reliable_event(&TestEvent(raw_tick / EVENT_EVERY_TICKS))
                    .unwrap();
            }
            writer
                .write_into_with_policy(tick, &captured, &mut packet, |id| {
                    let entity_index = server_ids
                        .iter()
                        .position(|candidate| *candidate == id)
                        .unwrap_or(usize::MAX);
                    EntityReplication {
                        relevant: true,
                        priority: if entity_index < PLAYER_COUNT {
                            10.0
                        } else {
                            1.0
                        },
                        max_update_interval_ticks: SNAPSHOT_EVERY_TICKS,
                        dormant: entity_index >= PLAYER_COUNT && raw_tick >= DEBRIS_REST_TICK,
                    }
                })
                .unwrap();
            encode_snapshot(&packet, &mut packet_bytes).unwrap();
            let stats = region.map(|region| region.change());
            if let Some(stats) = stats {
                server_allocations += stats.allocations;
                server_reallocations += stats.reallocations;
            }
            downstream_bytes += packet_bytes.len();
            for entity in &packet.entities {
                let entity_index = server_ids
                    .iter()
                    .position(|candidate| *candidate == entity.id)
                    .unwrap_or(usize::MAX);
                if entity_index < PLAYER_COUNT {
                    player_updates += 1;
                } else {
                    debris_updates += 1;
                }
            }
            server_link
                .send(Channel::Unreliable, &packet_bytes)
                .unwrap();
        }
    }

    let duration_seconds = f64::from(SOAK_TICKS) / 60.0;
    let bandwidth = downstream_bytes as f64 / duration_seconds;
    let player_updates_per_entity_second =
        player_updates as f64 / (PLAYER_COUNT as f64 * duration_seconds);
    let debris_updates_per_entity_second =
        debris_updates as f64 / (DEBRIS_COUNT as f64 * duration_seconds);
    println!(
        "downstream_before={PREVIOUS_TRANSFORM_CODEC_BANDWIDTH_BYTES_PER_SECOND:.1} B/s downstream_after={bandwidth:.1} B/s player_updates={player_updates_per_entity_second:.2}/entity/s debris_updates={debris_updates_per_entity_second:.2}/entity/s"
    );
    assert!(bandwidth <= MAX_BANDWIDTH_BYTES_PER_SECOND as f64);
    assert!(player_updates > 0);
    assert!(debris_updates > 0);
    assert_eq!(server_allocations, 0);
    assert_eq!(server_reallocations, 0);
    assert_eq!(client_allocations, 0);
    assert_eq!(client_reallocations, 0);
    assert_eq!(event_order.len(), (SOAK_TICKS / EVENT_EVERY_TICKS) as usize);
    for (sequence, received) in event_order.iter().enumerate() {
        assert_eq!(*received, u32::try_from(sequence).unwrap());
    }

    let server_final = capture_world(&server_world, None, Some(owner_id), bounds, &codecs).unwrap();
    let client_final = capture_world(&client_world, None, None, bounds, &codecs).unwrap();
    assert_eq!(server_final.len(), ENTITY_COUNT);
    assert_eq!(client_final.len(), ENTITY_COUNT);
    for (server, client) in server_final.iter().zip(&client_final) {
        assert_eq!(
            server, client,
            "replicated state diverged for {:?}",
            server.id
        );
    }
}

fn make_world(seed: u64) -> World {
    World::new(seed).unwrap()
}

fn spawn_scene(world: &mut World, ids: &mut Vec<engine_world::NetId>) {
    for index in 0..ENTITY_COUNT {
        let id = world.spawn().unwrap();
        world
            .insert(
                id,
                Position {
                    x: i32::try_from(index).unwrap() * 10,
                    y: 0,
                    z: 0,
                },
            )
            .unwrap();
        ids.push(id);
    }
}

fn move_scene(world: &mut World, ids: &[engine_world::NetId], tick: u32) {
    for (index, id) in ids.iter().enumerate() {
        let moving = if index < PLAYER_COUNT {
            tick < PLAYER_STOP_TICK
        } else {
            tick < DEBRIS_REST_TICK
        };
        if moving {
            let mut position = world.get_mut::<Position>(*id).unwrap();
            position.x = position.x.wrapping_add(1);
        }
    }
}
