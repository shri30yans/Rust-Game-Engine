#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    missing_docs
)]

mod common;

use common::TestInput;
use engine_core::Tick;
use engine_netcode::{
    Message, NetError, PacketAck, decode, encode,
    input::{
        INPUT_REDUNDANCY, InputBundle, InputFrame, InputHistory, InputReceiver, InputRepeater,
    },
};
use proptest::prelude::*;

#[test]
fn validate_failure_on_new_frame() {
    let err = InputFrame::new(
        Tick::new(1),
        TestInput {
            move_x: 0,
            buttons: 0x80,
        },
    )
    .expect_err("invalid buttons");
    assert!(matches!(err, NetError::InvalidLinkConditions(_)));
}

#[test]
fn missing_input_repeats_last_value_and_counts_loss() {
    let mut repeater = InputRepeater::new();
    let input = TestInput {
        move_x: 1,
        buttons: 0,
    };
    assert_eq!(repeater.for_tick(Some(input.clone())), Some(input.clone()));
    assert_eq!(repeater.for_tick(None), Some(input));
    assert_eq!(repeater.missing_ticks(), 1);
}

#[test]
fn input_bundle_carries_the_snapshot_ack_window() {
    let mut history = InputHistory::new();
    history
        .push(
            InputFrame::new(
                Tick::new(1),
                TestInput {
                    move_x: 0,
                    buttons: 0,
                },
            )
            .unwrap(),
        )
        .unwrap();
    let ack = PacketAck::with_previous(engine_netcode::PacketSequence::new(12), 0b101);
    history.acknowledge_snapshots(ack);
    assert_eq!(history.bundle().unwrap().snapshot_ack(), ack);
}

#[test]
fn validate_failure_on_decode() {
    #[derive(serde::Serialize)]
    struct WireFrame {
        tick: Tick,
        payload: TestInput,
    }
    #[derive(serde::Serialize)]
    struct WireBundle {
        frames: Vec<WireFrame>,
        snapshot_ack: PacketAck,
    }
    let bundle = WireBundle {
        frames: vec![WireFrame {
            tick: Tick::new(1),
            payload: TestInput {
                move_x: 0,
                buttons: 0x80,
            },
        }],
        snapshot_ack: PacketAck::NONE,
    };
    let bundle_bytes = postcard::to_allocvec(&bundle).unwrap();
    let mut datagram = Vec::new();
    datagram.push(3u8); // `Message::Inputs` variant index for this schema
    datagram.extend_from_slice(&bundle_bytes);
    let err = decode::<TestInput>(&datagram).expect_err("invalid payload");
    assert!(matches!(err, NetError::InvalidLinkConditions(_)));
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FatInput {
    data: [u8; 32],
}

impl engine_netcode::InputPayload for FatInput {
    const MAX_BYTES: usize = 8;

    fn validate(&self) -> Result<(), NetError> {
        Ok(())
    }
}

#[test]
fn payload_over_max_rejected_on_encode() {
    let frame = InputFrame::new(Tick::new(0), FatInput { data: [7; 32] }).unwrap();
    let mut history = InputHistory::<FatInput>::new();
    history.push(frame).unwrap();
    let msg = Message::Inputs(history.bundle().unwrap());
    let mut buf = Vec::new();
    let err = encode(&msg, &mut buf).expect_err("too large");
    assert!(matches!(err, NetError::PayloadTooLarge { .. }));
}

#[test]
fn history_rejects_non_increasing_ticks() {
    let mut history = InputHistory::new();
    history
        .push(
            InputFrame::new(
                Tick::new(2),
                TestInput {
                    move_x: 0,
                    buttons: 0,
                },
            )
            .unwrap(),
        )
        .unwrap();
    let err = history
        .push(
            InputFrame::new(
                Tick::new(2),
                TestInput {
                    move_x: 1,
                    buttons: 0,
                },
            )
            .unwrap(),
        )
        .expect_err("equal tick");
    match err {
        NetError::InputTickNotIncreasing { last, got } => {
            assert_eq!(last, Tick::new(2));
            assert_eq!(got, Tick::new(2));
        }
        other => panic!("unexpected {other:?}"),
    }
    let err = history
        .push(
            InputFrame::new(
                Tick::new(1),
                TestInput {
                    move_x: 1,
                    buttons: 0,
                },
            )
            .unwrap(),
        )
        .expect_err("older tick");
    match err {
        NetError::InputTickNotIncreasing { last, got } => {
            assert_eq!(last, Tick::new(2));
            assert_eq!(got, Tick::new(1));
        }
        other => panic!("unexpected {other:?}"),
    }
}

proptest! {
    #[test]
    fn receiver_yields_each_tick_once_under_loss(
        start in 0u32..100,
        steps in 1usize..40,
        drop_mask in prop::collection::vec(any::<bool>(), 1..40),
    ) {
        let mut history = InputHistory::new();
        let mut tick = start;
        for _ in 0..steps {
            history
                .push(InputFrame::new(Tick::new(tick), TestInput { move_x: 0, buttons: 0 }).unwrap())
                .unwrap();
            tick = tick.wrapping_add(1);
        }
        let mut receiver = InputReceiver::new();
        let mut seen = Vec::new();
        for &drop in &drop_mask {
            if drop {
                continue;
            }
            if let Some(bundle) = history.bundle() {
                for frame in receiver.accept(&bundle) {
                    seen.push(frame.tick().raw());
                }
            }
        }
        let mut deduped = seen.clone();
        deduped.sort_unstable();
        deduped.dedup();
        prop_assert_eq!(seen.len(), deduped.len(), "duplicate tick delivered");
        for window in deduped.windows(2) {
            prop_assert!(window[0] < window[1], "ticks not ascending");
        }
    }
}

#[test]
fn bundle_has_redundancy_shape() {
    let mut history = InputHistory::new();
    for t in 0..(INPUT_REDUNDANCY + 2) as u32 {
        history
            .push(
                InputFrame::new(
                    Tick::new(t),
                    TestInput {
                        move_x: 0,
                        buttons: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    }
    let bundle = history.bundle().unwrap();
    assert_eq!(bundle.frames().len(), INPUT_REDUNDANCY + 1);
}

#[test]
fn history_keeps_working_after_capacity() {
    let mut history = InputHistory::new();
    for t in 0..500u32 {
        history
            .push(
                InputFrame::new(
                    Tick::new(t),
                    TestInput {
                        move_x: 0,
                        buttons: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let bundle = history.bundle().expect("bundle after push");
        assert_eq!(
            bundle.frames().get(0).expect("newest frame").tick(),
            Tick::new(t)
        );
        assert!(bundle.frames().len() <= INPUT_REDUNDANCY + 1);
    }
}

#[test]
fn bundle_stops_at_a_tick_gap() {
    let mut history = InputHistory::new();
    for &t in &[0u32, 1, 2, 10] {
        history
            .push(
                InputFrame::new(
                    Tick::new(t),
                    TestInput {
                        move_x: 0,
                        buttons: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    }
    let bundle = history.bundle().unwrap();
    assert_eq!(bundle.frames().len(), 1);
    assert_eq!(
        bundle.frames().get(0).expect("newest frame").tick(),
        Tick::new(10)
    );
}

#[test]
fn decoding_rejects_more_frames_than_the_redundancy_bound() {
    #[derive(serde::Serialize)]
    struct WireBundle {
        frames: Vec<InputFrame<TestInput>>,
        snapshot_ack: PacketAck,
    }

    let frames = (0..=INPUT_REDUNDANCY + 1)
        .map(|tick| {
            InputFrame::new(
                Tick::new(u32::try_from(tick).expect("small frame index")),
                TestInput {
                    move_x: 0,
                    buttons: 0,
                },
            )
            .expect("valid frame")
        })
        .collect();
    let bytes = postcard::to_allocvec(&WireBundle {
        frames,
        snapshot_ack: PacketAck::NONE,
    })
    .expect("wire bundle");

    assert!(postcard::from_bytes::<InputBundle<TestInput>>(&bytes).is_err());
}
