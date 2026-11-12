//! The QUIC transport over real localhost UDP sockets: connect, both channels, the engine
//! handshake, certificate pinning, capacity, disconnect and reconnect.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing
)]

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use common::TestInput;
use engine_netcode::quic::{
    CertificateHash, ClientConfig, ClientId, ClientState, CloseReason, QuicClient, QuicServer,
    ServerConfig, ServerEvent, ServerIdentity,
};
use engine_netcode::{
    Channel, ClientHandshake, GameId, GameProtocol, GameVersion, MAX_DATAGRAM_BYTES, Message,
    NetError, ServerHandshake, ServerTransport, Transport, decode, encode,
};

const DEADLINE: Duration = Duration::from_secs(10);

fn server(max_clients: u32) -> QuicServer {
    let identity = ServerIdentity::generate(&["localhost"]).unwrap();
    let bind: SocketAddr = (Ipv4Addr::LOCALHOST, 0).into();
    QuicServer::bind(ServerConfig::new(bind, max_clients, identity)).unwrap()
}

fn client(server: &QuicServer) -> QuicClient {
    QuicClient::connect(ClientConfig::new(
        server.local_addr(),
        server.certificate_hash(),
    ))
    .unwrap()
}

/// Polls both ends until `done` says so, collecting server events; fails after the deadline.
fn pump(
    server: &mut QuicServer,
    clients: &mut [&mut QuicClient],
    events: &mut Vec<ServerEvent>,
    mut done: impl FnMut(&[ServerEvent], &[&mut QuicClient]) -> bool,
) {
    let start = Instant::now();
    let mut buffer = Vec::new();
    while !done(events, clients) {
        assert!(
            start.elapsed() < DEADLINE,
            "timed out; events so far: {events:?}"
        );
        while let Some(event) = server.recv(&mut buffer) {
            events.push(event);
        }
        for client in clients.iter_mut() {
            while let Ok(Some(_)) = client.recv(&mut buffer) {}
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn connected(events: &[ServerEvent]) -> Vec<ClientId> {
    events
        .iter()
        .filter_map(|event| match event {
            ServerEvent::Connected(id) => Some(*id),
            _ => None,
        })
        .collect()
}

#[test]
fn certificate_hash_round_trips_as_hex_and_rejects_bad_text() {
    let hash = CertificateHash::of(b"certificate bytes");
    let text = hash.to_string();
    assert_eq!(text.len(), 64);
    assert_eq!(text.parse::<CertificateHash>().unwrap(), hash);
    assert!("abc".parse::<CertificateHash>().is_err());
    assert!("zz".repeat(32).parse::<CertificateHash>().is_err());
}

#[test]
fn two_clients_exchange_reliable_and_unreliable_messages_both_ways() {
    let mut server = server(4);
    let mut a = client(&server);
    let mut b = client(&server);
    let mut events = Vec::new();
    pump(
        &mut server,
        &mut [&mut a, &mut b],
        &mut events,
        |events, clients| {
            connected(events).len() == 2
                && clients
                    .iter()
                    .all(|client| *client.state() == ClientState::Connected)
        },
    );
    let ids = connected(&events);
    let first_id = *ids.first().unwrap();
    let second_id = *ids.get(1).unwrap();
    assert_ne!(first_id, second_id);

    const RELIABLE: u32 = 200;
    const UNRELIABLE: u32 = 1000;
    for n in 0..RELIABLE {
        a.send(Channel::Reliable, &n.to_le_bytes()).unwrap();
        server
            .send(first_id, Channel::Reliable, &n.to_le_bytes())
            .unwrap();
        server
            .send(second_id, Channel::Reliable, &n.to_le_bytes())
            .unwrap();
    }
    let big = vec![9_u8; MAX_DATAGRAM_BYTES];
    for _ in 0..UNRELIABLE {
        b.send(Channel::Unreliable, &big).unwrap();
    }

    let start = Instant::now();
    let mut buffer = Vec::new();
    let mut server_reliable = Vec::new();
    let mut server_unreliable = 0_u32;
    // The two handshakes race, so which id is `a` is learned from the traffic: `a` sends only
    // reliable messages and `b` only unreliable ones.
    let mut reliable_sender = None;
    let mut unreliable_sender = None;
    let mut client_reliable = [Vec::new(), Vec::new()];
    while (server_reliable.len() as u32) < RELIABLE
        || client_reliable
            .iter()
            .any(|got| (got.len() as u32) < RELIABLE)
        || server_unreliable + (server.dropped_unreliable() as u32) < UNRELIABLE * 9 / 10
    {
        assert!(start.elapsed() < DEADLINE, "timed out");
        while let Some(event) = server.recv(&mut buffer) {
            match event {
                ServerEvent::Message(id, Channel::Reliable) => {
                    assert_eq!(*reliable_sender.get_or_insert(id), id);
                    server_reliable.push(u32::from_le_bytes(buffer[..4].try_into().unwrap()));
                }
                ServerEvent::Message(id, Channel::Unreliable) => {
                    assert_eq!(*unreliable_sender.get_or_insert(id), id);
                    assert_eq!(buffer, big);
                    server_unreliable += 1;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        for (index, client) in [&mut a, &mut b].into_iter().enumerate() {
            while let Some(channel) = client.recv(&mut buffer).unwrap() {
                assert_eq!(channel, Channel::Reliable);
                client_reliable[index].push(u32::from_le_bytes(buffer[..4].try_into().unwrap()));
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let in_order: Vec<u32> = (0..RELIABLE).collect();
    assert_eq!(server_reliable, in_order);
    assert_eq!(client_reliable[0], in_order);
    assert_eq!(client_reliable[1], in_order);
    assert_ne!(reliable_sender, unreliable_sender);
    println!(
        "unreliable: {server_unreliable} of {UNRELIABLE} delivered, {} dropped by a full queue",
        server.dropped_unreliable()
    );
}

#[test]
fn the_engine_handshake_runs_over_quic_and_refuses_the_wrong_game() {
    let game = GameProtocol {
        id: GameId::new(7),
        version: GameVersion::new(1),
    };
    let wrong = GameProtocol {
        id: GameId::new(7),
        version: GameVersion::new(2),
    };
    let mut server = server(4);
    let mut gate = ServerHandshake::new(game, 4);
    let mut events = Vec::new();
    let mut outcomes = Vec::new();
    for (offered, nonce) in [(game, 1_u64), (wrong, 2)] {
        let mut quic = client(&server);
        pump(&mut server, &mut [&mut quic], &mut events, |_, clients| {
            clients
                .iter()
                .all(|client| *client.state() == ClientState::Connected)
        });
        let (mut handshake, hello) = ClientHandshake::start::<TestInput>(offered, nonce);
        let mut bytes = Vec::new();
        encode(&hello, &mut bytes).unwrap();
        quic.send(Channel::Reliable, &bytes).unwrap();

        let start = Instant::now();
        let mut buffer = Vec::new();
        let reply = loop {
            assert!(start.elapsed() < DEADLINE, "timed out");
            while let Some(event) = server.recv(&mut buffer) {
                if let ServerEvent::Message(id, Channel::Reliable) = event {
                    let message = decode::<TestInput>(&buffer).unwrap();
                    let answer = gate.on_client_message(message).unwrap();
                    let mut out = Vec::new();
                    encode(&answer, &mut out).unwrap();
                    server.send(id, Channel::Reliable, &out).unwrap();
                }
            }
            if let Some(Channel::Reliable) = quic.recv(&mut buffer).unwrap() {
                break decode::<TestInput>(&buffer).unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        outcomes.push(matches!(reply, Message::Reject { .. }));
        handshake.on_server_message(reply).unwrap();
        assert_eq!(handshake.is_connected(), offered == game);
    }
    assert_eq!(outcomes, vec![false, true]);
}

#[test]
fn a_client_pinning_another_certificate_never_connects() {
    let mut server = server(4);
    let other = ServerIdentity::generate(&["localhost"]).unwrap();
    let mut quic = QuicClient::connect(ClientConfig::new(
        server.local_addr(),
        other.certificate_hash(),
    ))
    .unwrap();
    let mut events = Vec::new();
    pump(&mut server, &mut [&mut quic], &mut events, |_, clients| {
        clients
            .iter()
            .all(|client| matches!(client.state(), ClientState::Closed(_)))
    });
    assert!(connected(&events).is_empty());
    assert_eq!(
        quic.send(Channel::Reliable, b"x"),
        Err(NetError::ChannelClosed)
    );
}

#[test]
fn a_full_server_refuses_the_extra_client() {
    let mut server = server(1);
    let mut first = client(&server);
    let mut events = Vec::new();
    pump(&mut server, &mut [&mut first], &mut events, |events, _| {
        connected(events).len() == 1
    });
    let mut second = client(&server);
    pump(
        &mut server,
        &mut [&mut second],
        &mut events,
        |_, clients| {
            clients
                .iter()
                .all(|client| matches!(client.state(), ClientState::Closed(_)))
        },
    );
    assert_eq!(*second.state(), ClientState::Closed(CloseReason::Refused));
    assert_eq!(connected(&events).len(), 1);
}

#[test]
fn a_dropped_client_is_reported_and_a_reconnect_gets_a_new_id() {
    let mut server = server(1);
    let mut first = client(&server);
    let mut events = Vec::new();
    pump(&mut server, &mut [&mut first], &mut events, |events, _| {
        connected(events).len() == 1
    });
    let first_id = *connected(&events).first().unwrap();
    drop(first);
    pump(&mut server, &mut [], &mut events, |events, _| {
        events
            .iter()
            .any(|event| matches!(event, ServerEvent::Disconnected(id, _) if *id == first_id))
    });
    assert_eq!(
        server.send(first_id, Channel::Reliable, b"x"),
        Err(NetError::ChannelClosed)
    );

    let mut again = client(&server);
    pump(&mut server, &mut [&mut again], &mut events, |events, _| {
        connected(events).len() == 2
    });
    let second_id = *connected(&events).get(1).unwrap();
    assert_eq!(second_id.index(), first_id.index());
    assert_ne!(second_id, first_id);
}

#[test]
fn oversized_and_early_sends_are_typed_errors() {
    let mut server = server(1);
    let mut quic = client(&server);
    assert_eq!(
        quic.send(Channel::Unreliable, b"x"),
        Err(NetError::NotConnected)
    );
    let mut events = Vec::new();
    pump(&mut server, &mut [&mut quic], &mut events, |_, clients| {
        clients
            .iter()
            .all(|client| *client.state() == ClientState::Connected)
    });
    let too_big = vec![0_u8; MAX_DATAGRAM_BYTES + 1];
    assert!(matches!(
        quic.send(Channel::Unreliable, &too_big),
        Err(NetError::PayloadOverTransportLimit { .. })
    ));
}
