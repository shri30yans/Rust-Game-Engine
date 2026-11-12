//! Steady-state `encode` and input acceptance must not allocate.
//!
//! One `#[test]` only: the allocator counter is process-wide, so parallel tests would pollute it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::TestInput;
use engine_core::Tick;
use engine_netcode::{
    Message, encode,
    input::{InputFrame, InputHistory, InputReceiver},
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn encode_inputs_no_alloc_after_warm_up() {
    let mut history = InputHistory::new();
    for t in 0..5u32 {
        history
            .push(
                InputFrame::new(
                    Tick::new(t),
                    TestInput {
                        move_x: 1,
                        buttons: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    }
    let bundle = history.bundle().unwrap();
    let msg = Message::Inputs(bundle);
    let mut buf = Vec::new();
    encode(&msg, &mut buf).unwrap();

    encode(&msg, &mut buf).unwrap();
    let region = Region::new(GLOBAL);
    encode(&msg, &mut buf).unwrap();
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "encode allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "encode reallocated: {stats:?}");
}

fn accept_iterator_allocates_nothing() {
    let mut history = InputHistory::new();
    for t in 0..8u32 {
        history
            .push(
                InputFrame::new(
                    Tick::new(t),
                    TestInput {
                        move_x: 1,
                        buttons: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    }
    let bundle = history.bundle().unwrap();
    let mut receiver = InputReceiver::new();
    let mut iter = receiver.accept(&bundle);
    let _ = iter.next();

    let region = Region::new(GLOBAL);
    let mut iter = receiver.accept(&bundle);
    for _frame in iter.by_ref() {}
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "accept iterator allocated: {stats:?}");
    assert_eq!(
        stats.reallocations, 0,
        "accept iterator reallocated: {stats:?}"
    );
}

#[test]
fn hot_paths_allocate_nothing() {
    encode_inputs_no_alloc_after_warm_up();
    accept_iterator_allocates_nothing();
}
