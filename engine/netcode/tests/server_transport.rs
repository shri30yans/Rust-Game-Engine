//! The server-side transport contract on the loopback hub, and the session and possess messages.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

mod common;

use common::TestInput;
use engine_core::Tick;
use engine_netcode::{
    Channel, ClientId, CloseReason, LinkConditions, LoopbackHub, MAX_SESSION_BYTES, Message,
    MessageKind, NetError, ServerEvent, ServerTransport, Transport, decode, encode,
};
use engine_world::World;

fn drain(hub: &mut LoopbackHub, out: &mut Vec<u8>) -> Vec<(ServerEvent, Vec<u8>)> {
    let mut events = Vec::new();
    while let Some(event) = hub.recv(out) {
        events.push((event, out.clone()));
    }
    events
}

#[test]
fn clients_are_announced_then_heard_in_order_with_their_own_ids() {
    let mut hub = LoopbackHub::new(4, 9);
    let mut a = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .unwrap();
    let mut b = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .unwrap();
    let mut out = Vec::new();
    let events = drain(&mut hub, &mut out);
    let ids: Vec<ClientId> = events
        .iter()
        .map(|(event, _)| match event {
            ServerEvent::Connected(id) => *id,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);

    for n in 0..5_u8 {
        a.send(Channel::Reliable, &[1, n]).unwrap();
        b.send(Channel::Reliable, &[2, n]).unwrap();
    }
    let events = drain(&mut hub, &mut out);
    let mut from = [Vec::new(), Vec::new()];
    for (event, bytes) in events {
        let ServerEvent::Message(id, Channel::Reliable) = event else {
            panic!("unexpected {event:?}");
        };
        let who = usize::from(bytes[0] - 1);
        assert_eq!(id, ids[who], "a message carries its sender's id");
        from[who].push(bytes[1]);
    }
    assert_eq!(from, [vec![0, 1, 2, 3, 4], vec![0, 1, 2, 3, 4]]);

    hub.send(ids[1], Channel::Reliable, b"hello b").unwrap();
    let mut got = Vec::new();
    assert_eq!(b.recv(&mut got).unwrap(), Some(Channel::Reliable));
    assert_eq!(got, b"hello b");
    assert_eq!(a.recv(&mut got).unwrap(), None);
}

#[test]
fn reliable_traffic_survives_latency_and_loss_and_arrives_when_due() {
    let lossy = LinkConditions::new(5, 2, 200, 100).unwrap();
    let mut hub = LoopbackHub::new(1, 3);
    let mut client = hub.connect(lossy, lossy).unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        hub.recv(&mut out),
        Some(ServerEvent::Connected(_))
    ));
    for n in 0..50_u8 {
        client.send(Channel::Reliable, &[n]).unwrap();
    }
    assert!(
        hub.recv(&mut out).is_none(),
        "nothing arrives before the latency"
    );
    let mut got = Vec::new();
    for tick in 1..=20 {
        hub.advance_to(Tick::new(tick));
        while let Some(ServerEvent::Message(_, Channel::Reliable)) = hub.recv(&mut out) {
            got.push(out[0]);
        }
    }
    assert_eq!(got, (0..50).collect::<Vec<u8>>());
}

#[test]
fn drops_both_ways_are_reported_and_slots_come_back_with_a_new_generation() {
    let mut hub = LoopbackHub::new(1, 1);
    let client = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .unwrap();
    assert!(matches!(
        hub.connect(LinkConditions::PERFECT, LinkConditions::PERFECT),
        Err(NetError::ServerFull)
    ));
    let mut out = Vec::new();
    let Some(ServerEvent::Connected(first)) = hub.recv(&mut out) else {
        panic!("no connect");
    };
    drop(client);
    assert_eq!(
        hub.recv(&mut out),
        Some(ServerEvent::Disconnected(
            first,
            CloseReason::ClosedByPeer(0)
        ))
    );
    assert_eq!(
        hub.send(first, Channel::Reliable, b"x"),
        Err(NetError::ChannelClosed)
    );
    assert_eq!(hub.connected(), 0);

    let mut again = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .unwrap();
    let Some(ServerEvent::Connected(second)) = hub.recv(&mut out) else {
        panic!("no reconnect");
    };
    assert_eq!(second.index(), first.index());
    assert_ne!(second, first);

    hub.disconnect(second).unwrap();
    assert_eq!(
        hub.recv(&mut out),
        Some(ServerEvent::Disconnected(
            second,
            CloseReason::ClosedLocally
        ))
    );
    assert_eq!(again.recv(&mut out), Err(NetError::ChannelClosed));
    assert_eq!(hub.disconnect(second), Err(NetError::ChannelClosed));
}

#[test]
fn the_same_seed_gives_the_same_deliveries() {
    let run = || {
        let lossy = LinkConditions::new(3, 4, 300, 300).unwrap();
        let mut hub = LoopbackHub::new(2, 42);
        let mut a = hub.connect(lossy, lossy).unwrap();
        let mut b = hub.connect(lossy, lossy).unwrap();
        let mut log = Vec::new();
        let mut out = Vec::new();
        for tick in 0..40_u32 {
            let byte = u8::try_from(tick).unwrap();
            a.send(Channel::Unreliable, &[byte]).unwrap();
            b.send(Channel::Unreliable, &[byte | 0x80]).unwrap();
            hub.advance_to(Tick::new(tick));
            while let Some(event) = hub.recv(&mut out) {
                log.push((format!("{event:?}"), out.clone()));
            }
        }
        log
    };
    assert_eq!(run(), run());
}

#[test]
fn session_and_possess_round_trip_on_the_reliable_channel_within_their_caps() {
    let mut world = World::new(1).unwrap();
    let pawn = world.spawn().unwrap();
    let mut buf = Vec::new();
    for message in [
        Message::<TestInput>::Session(vec![7; 4_000]),
        Message::Possess { pawn },
    ] {
        assert_eq!(message.channel(), Channel::Reliable);
        encode(&message, &mut buf).unwrap();
        assert_eq!(decode::<TestInput>(&buf).unwrap(), message);
    }
    let too_big = Message::<TestInput>::Session(vec![0; MAX_SESSION_BYTES]);
    assert!(matches!(
        encode(&too_big, &mut buf),
        Err(NetError::MessageTooLarge {
            kind: MessageKind::Session,
            ..
        })
    ));
}
