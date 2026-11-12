//! Determinism of the sandbox: the same command script must produce the same state hash on every
//! tick, and a different script must produce a different final hash. Runs headless.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::ViewAngles;
use common::{FACING_PLUS_X, new_app, place, tick};
use glam::Vec3;
use sandbox::{Axis, PlayerCommand};

const SCRIPT_TICKS: u32 = 600;

/// The view at `frame`, turning at `rate` radians per tick from yaw 0. Built through the validated
/// constructor, so the script can only produce views the engine's mouse look could produce.
fn sweeping_view(frame: u32, rate: f32) -> ViewAngles {
    let ticks = f32::from(u16::try_from(frame % 1024).expect("below 1024"));
    ViewAngles::new(ticks * rate, 0.0).expect("finite view")
}

/// A mixed script: walks, turns the view, strafes and jumps at varying intervals.
fn mixed(frame: u32) -> PlayerCommand {
    let forward = if (frame / 60).is_multiple_of(2) {
        Axis::Positive
    } else {
        Axis::Zero
    };
    let strafe = if frame % 97 < 20 {
        Axis::Negative
    } else {
        Axis::Zero
    };
    let jump = frame.is_multiple_of(75);
    PlayerCommand::new(strafe, forward, jump, sweeping_view(frame, 0.02))
}

/// The same kinds of input at different phases, turning the other way, so the trajectory differs
/// from `mixed`.
fn shifted(frame: u32) -> PlayerCommand {
    let forward = if (frame / 45).is_multiple_of(2) {
        Axis::Positive
    } else {
        Axis::Zero
    };
    let strafe = if frame % 83 < 30 {
        Axis::Positive
    } else {
        Axis::Zero
    };
    let jump = frame % 76 == 5;
    PlayerCommand::new(strafe, forward, jump, sweeping_view(frame, -0.015))
}

/// Runs `script` for `SCRIPT_TICKS` ticks from a fixed start and returns the hash after each tick.
fn record(script: fn(u32) -> PlayerCommand) -> Vec<u64> {
    let mut app = new_app();
    place(&mut app, Vec3::new(-8.0, 1.0, 4.0), FACING_PLUS_X);
    let mut hashes = Vec::with_capacity(SCRIPT_TICKS as usize);
    for frame in 0..SCRIPT_TICKS {
        tick(&mut app, script(frame));
        hashes.push(app.state_hash().expect("state hash"));
    }
    hashes
}

#[test]
fn same_script_gives_identical_hashes_every_tick() {
    let first = record(mixed);
    let second = record(mixed);
    assert_eq!(first.len(), SCRIPT_TICKS as usize);
    assert_eq!(
        first, second,
        "two runs of the same script must hash identically tick by tick"
    );
    assert_eq!(
        first.last().copied(),
        Some(11_303_523_162_233_574_502),
        "the 600-tick sandbox script matches the pack-backed sandbox level"
    );
}

#[test]
fn the_hash_follows_the_simulation() {
    let hashes = record(mixed);
    let distinct = {
        let mut seen = hashes.clone();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    // A hash that never changes would make the determinism check vacuous.
    assert!(
        distinct > SCRIPT_TICKS as usize / 2,
        "hash should change with the state, got {distinct} distinct values"
    );
}

#[test]
fn a_different_script_gives_a_different_final_hash() {
    let mixed_hashes = record(mixed);
    let shifted_hashes = record(shifted);
    let mixed_last = mixed_hashes.last().copied().expect("mixed run has ticks");
    let shifted_last = shifted_hashes
        .last()
        .copied()
        .expect("shifted run has ticks");
    assert_ne!(
        mixed_last, shifted_last,
        "different input must lead to a different final state"
    );
}
