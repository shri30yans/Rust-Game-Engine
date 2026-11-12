#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use common::TestInput;
use engine_netcode::{
    ClientHandshake, GameId, GameProtocol, GameVersion, Message, ProtocolVersion, RejectReason,
    ServerHandshake,
};

const GAME: GameProtocol = GameProtocol {
    id: GameId::new(7),
    version: GameVersion::new(3),
};

#[test]
fn version_mismatch_rejects() {
    let mut server = ServerHandshake::new(GAME, 4);
    let msg = server.on_hello::<TestInput>(ProtocolVersion::new(999), GAME, 1);
    match msg {
        Message::Reject {
            reason: RejectReason::VersionMismatch,
        } => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn server_full_rejects() {
    let mut server = ServerHandshake::new(GAME, 2);
    assert!(matches!(
        server.on_hello::<TestInput>(ProtocolVersion::CURRENT, GAME, 1),
        Message::Welcome { .. }
    ));
    assert!(matches!(
        server.on_hello::<TestInput>(ProtocolVersion::CURRENT, GAME, 2),
        Message::Welcome { .. }
    ));
    assert!(matches!(
        server.on_hello::<TestInput>(ProtocolVersion::CURRENT, GAME, 3),
        Message::Reject {
            reason: RejectReason::ServerFull
        }
    ));
}

#[test]
fn wrong_game_id_rejects() {
    let mut server = ServerHandshake::new(GAME, 4);
    let wrong = GameProtocol {
        id: GameId::new(8),
        version: GameVersion::new(3),
    };
    assert!(matches!(
        server.on_hello::<TestInput>(ProtocolVersion::CURRENT, wrong, 1),
        Message::Reject {
            reason: RejectReason::WrongGame
        }
    ));
}

#[test]
fn wrong_game_version_rejects() {
    let mut server = ServerHandshake::new(GAME, 4);
    let wrong = GameProtocol {
        id: GameId::new(7),
        version: GameVersion::new(99),
    };
    assert!(matches!(
        server.on_hello::<TestInput>(ProtocolVersion::CURRENT, wrong, 1),
        Message::Reject {
            reason: RejectReason::WrongGame
        }
    ));
}

#[test]
fn happy_path_assigns_increasing_client_index() {
    let mut server = ServerHandshake::new(GAME, 4);
    let mut indices = Vec::new();
    for nonce in 0..3u64 {
        match server.on_hello::<TestInput>(ProtocolVersion::CURRENT, GAME, nonce) {
            Message::Welcome { client_index, .. } => indices.push(client_index.raw()),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(indices, [0, 1, 2]);
    let (_client, hello) = ClientHandshake::start::<TestInput>(GAME, 5);
    match hello {
        Message::Hello { version, game, .. } => {
            assert_eq!(version, ProtocolVersion::CURRENT);
            assert_eq!(game, GAME);
        }
        _ => panic!("expected hello"),
    }
}
