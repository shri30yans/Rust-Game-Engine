#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::assertions_on_constants,
    missing_docs
)]

mod common;

use common::TestInput;
use engine_core::Tick;
use engine_netcode::{
    ClientIndex, GameId, GameProtocol, GameVersion, MAX_INPUTS_BYTES, Message, MessageKind,
    NetError, ProtocolVersion, RejectReason, decode, encode,
    input::{InputFrame, InputHistory},
};
use proptest::prelude::*;

fn hello_msg() -> Message<TestInput> {
    Message::Hello {
        version: ProtocolVersion::CURRENT,
        game: GameProtocol {
            id: GameId::new(1),
            version: GameVersion::new(2),
        },
        client_nonce: 99,
    }
}

fn bundle_msg() -> Message<TestInput> {
    let mut history = InputHistory::new();
    for t in 0..3u32 {
        let frame = InputFrame::new(
            Tick::new(t),
            TestInput {
                move_x: 1,
                buttons: 0,
            },
        )
        .unwrap();
        history.push(frame).unwrap();
    }
    Message::Inputs(history.bundle().unwrap())
}

proptest! {
    #[test]
    fn round_trip_hello(client_nonce in any::<u64>(), game_raw in any::<u64>(), ver in any::<u32>()) {
        let msg = Message::Hello {
            version: ProtocolVersion::CURRENT,
            game: GameProtocol { id: GameId::new(game_raw), version: GameVersion::new(ver) },
            client_nonce,
        };
        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();
        let back: Message<TestInput> = decode(&buf).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn round_trip_welcome(index in any::<u8>()) {
        let msg = Message::Welcome {
            version: ProtocolVersion::CURRENT,
            client_index: ClientIndex::new(index),
        };
        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();
        let back: Message<TestInput> = decode(&buf).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn round_trip_reject(reason in prop_oneof![
        Just(RejectReason::VersionMismatch),
        Just(RejectReason::WrongGame),
        Just(RejectReason::ServerFull),
    ]) {
        let msg = Message::Reject { reason };
        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();
        let back: Message<TestInput> = decode(&buf).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn round_trip_inputs(frames in 1usize..6, move_x in any::<i8>(), buttons in 0u8..0x80) {
        let mut history = InputHistory::new();
        for t in 0..frames as u32 {
            let frame = InputFrame::new(
                Tick::new(t),
                TestInput { move_x, buttons },
            ).unwrap();
            history.push(frame).unwrap();
        }
        let msg = Message::Inputs(history.bundle().unwrap());
        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();
        let back: Message<TestInput> = decode(&buf).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn decode_arbitrary_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let _ = decode::<TestInput>(&bytes);
    }
}

#[test]
fn oversized_inputs_rejected() {
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct WideInput {
        data: Vec<u8>,
    }
    impl engine_netcode::InputPayload for WideInput {
        const MAX_BYTES: usize = 400;
        fn validate(&self) -> Result<(), NetError> {
            Ok(())
        }
    }
    let mut history = InputHistory::<WideInput>::new();
    for t in 0..5u32 {
        history
            .push(InputFrame::new(Tick::new(t), WideInput { data: vec![1; 105] }).unwrap())
            .unwrap();
    }
    let msg = Message::Inputs(history.bundle().unwrap());
    let mut buf = Vec::new();
    let err = encode(&msg, &mut buf).expect_err("expected cap");
    match err {
        NetError::MessageTooLarge {
            kind: MessageKind::Inputs,
            ..
        } => {}
        other => panic!("unexpected err: {other:?}"),
    }
    assert!(buf.is_empty());
    assert!(MAX_INPUTS_BYTES < 4096);
}

#[test]
fn hello_round_trip_smoke() {
    let mut buf = Vec::new();
    encode(&hello_msg(), &mut buf).unwrap();
    let back: Message<TestInput> = decode(&buf).unwrap();
    assert_eq!(back, hello_msg());
}

#[test]
fn bundle_round_trip_smoke() {
    let mut buf = Vec::new();
    encode(&bundle_msg(), &mut buf).unwrap();
    let back: Message<TestInput> = decode(&buf).unwrap();
    assert_eq!(back, bundle_msg());
}
