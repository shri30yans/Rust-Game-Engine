//! View angles wrap and clamp, and the look inbox accumulates across threads.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::thread;

use app::{AppError, LookDelta, LookInbox, ViewAngles};
use proptest::prelude::*;

const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.001;
const TOLERANCE: f32 = 1.0e-3;

fn delta(yaw: f32, pitch: f32) -> LookDelta {
    LookDelta::new(yaw, pitch).expect("finite delta")
}

/// Distance between two yaw values on the circle.
fn yaw_gap(a: f32, b: f32) -> f32 {
    let direct = (a - b).rem_euclid(TAU);
    direct.min(TAU - direct)
}

#[test]
fn view_angles_reject_nan_and_infinity() {
    assert!(matches!(
        ViewAngles::new(f32::NAN, 0.0),
        Err(AppError::NonFiniteAngle)
    ));
    assert!(matches!(
        ViewAngles::new(0.0, f32::INFINITY),
        Err(AppError::NonFiniteAngle)
    ));
    assert!(matches!(
        ViewAngles::new(f32::NEG_INFINITY, 0.0),
        Err(AppError::NonFiniteAngle)
    ));
    assert!(matches!(
        LookDelta::new(0.0, f32::NAN),
        Err(AppError::NonFiniteAngle)
    ));
}

#[test]
fn pitch_clamps_at_the_limit_for_large_deltas() {
    let up = ViewAngles::new(0.0, 0.0)
        .expect("view")
        .turned(delta(0.0, 10.0));
    let down = ViewAngles::new(0.0, 0.0)
        .expect("view")
        .turned(delta(0.0, -10.0));

    assert!((up.pitch() - PITCH_LIMIT).abs() < 1.0e-6);
    assert!((down.pitch() + PITCH_LIMIT).abs() < 1.0e-6);
}

#[test]
fn yaw_wraps_past_pi() {
    let view = ViewAngles::new(PI + 0.1, 0.0).expect("view");
    assert!(
        (view.yaw() - (-PI + 0.1)).abs() < 1.0e-5,
        "got {}",
        view.yaw()
    );

    let turned = ViewAngles::new(PI - 0.05, 0.0)
        .expect("view")
        .turned(delta(0.2, 0.0));
    assert!(
        (turned.yaw() - (-PI + 0.15)).abs() < 1.0e-5,
        "got {}",
        turned.yaw()
    );
}

#[test]
fn look_delta_zero_is_the_identity() {
    let view = ViewAngles::new(0.3, 0.2).expect("view");
    assert_eq!(view.turned(LookDelta::ZERO), view);
    assert_eq!(LookDelta::default(), LookDelta::ZERO);
}

proptest! {
    #[test]
    fn turned_keeps_yaw_wrapped_and_pitch_clamped(
        yaw in -100.0f32..100.0,
        pitch in -100.0f32..100.0,
        start_yaw in -100.0f32..100.0,
        start_pitch in -100.0f32..100.0,
    ) {
        let view = ViewAngles::new(start_yaw, start_pitch).expect("view");
        let turned = view.turned(delta(yaw, pitch));

        prop_assert!(turned.yaw() > -PI && turned.yaw() <= PI, "yaw {}", turned.yaw());
        prop_assert!(turned.pitch().abs() <= PITCH_LIMIT, "pitch {}", turned.pitch());
    }

    #[test]
    fn turning_twice_equals_turning_by_the_sum_when_pitch_stays_in_range(
        a_yaw in -100.0f32..100.0,
        b_yaw in -100.0f32..100.0,
        a_pitch in -0.5f32..0.5,
        b_pitch in -0.5f32..0.5,
        start_yaw in -100.0f32..100.0,
        start_pitch in -0.5f32..0.5,
    ) {
        let start = ViewAngles::new(start_yaw, start_pitch).expect("view");
        let stepwise = start
            .turned(delta(a_yaw, a_pitch))
            .turned(delta(b_yaw, b_pitch));
        let at_once = start.turned(delta(a_yaw + b_yaw, a_pitch + b_pitch));

        prop_assert!(yaw_gap(stepwise.yaw(), at_once.yaw()) < TOLERANCE);
        prop_assert!((stepwise.pitch() - at_once.pitch()).abs() < TOLERANCE);
    }
}

#[test]
fn inbox_add_then_peek_keeps_the_total_and_drain_resets_it() {
    let inbox = LookInbox::new();
    inbox.add(delta(0.5, 0.25)).expect("add");
    inbox.add(delta(0.25, -0.5)).expect("add");

    let peeked = inbox.peek().expect("peek");
    assert!((peeked.yaw() - 0.75).abs() < 1.0e-6);
    assert!((peeked.pitch() + 0.25).abs() < 1.0e-6);
    assert_eq!(
        inbox.peek().expect("peek again"),
        peeked,
        "peek must not reset"
    );

    assert_eq!(inbox.drain().expect("drain"), peeked);
    assert_eq!(inbox.peek().expect("peek after drain"), LookDelta::ZERO);
}

#[test]
fn inbox_overflow_to_infinity_is_an_error_and_keeps_the_total() {
    let inbox = LookInbox::new();
    inbox.add(delta(f32::MAX, 0.0)).expect("first MAX fits");

    let overflow = inbox.add(delta(f32::MAX, 0.0));

    assert!(matches!(overflow, Err(AppError::NonFiniteAngle)));
    assert_eq!(inbox.peek().expect("peek"), delta(f32::MAX, 0.0));
}

#[test]
fn inbox_accumulates_adds_from_two_threads() {
    let inbox = LookInbox::new();
    let turner = inbox.clone();
    let other = thread::spawn(move || {
        for _ in 0..100 {
            turner.add(delta(1.0, 0.0)).expect("add from thread");
        }
    });
    for _ in 0..100 {
        inbox.add(delta(0.5, 0.0)).expect("add from main");
    }
    other.join().expect("thread joined");

    let total = inbox.drain().expect("drain");
    assert!((total.yaw() - 150.0).abs() < 1.0e-3, "got {}", total.yaw());
}
