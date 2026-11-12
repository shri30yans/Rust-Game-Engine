//! Fixed-step clock: rate validation, exact stepping, the catch-up cap and the
//! accounting invariants that hold for any frame-time sequence.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{AppError, FixedClock, TickRate};
use proptest::prelude::*;

const STEP_60_HZ: u64 = 16_666_666;

#[test]
fn tick_rate_rejects_zero_and_above_1000() {
    assert!(matches!(
        TickRate::new(0),
        Err(AppError::InvalidTickRate(0))
    ));
    assert!(matches!(
        TickRate::new(1001),
        Err(AppError::InvalidTickRate(1001))
    ));
    assert_eq!(TickRate::new(1).expect("1 Hz is valid").hz(), 1);
    assert_eq!(TickRate::new(1000).expect("1000 Hz is valid").hz(), 1000);
}

#[test]
fn tick_rate_step_and_dt() {
    let rate = TickRate::new(60).expect("valid rate");
    assert_eq!(rate.step_nanos(), STEP_60_HZ);
    assert!((rate.dt_seconds() - 1.0 / 60.0).abs() < f32::EPSILON);
}

#[test]
fn clock_rejects_zero_catch_up() {
    let rate = TickRate::new(60).expect("valid rate");
    assert!(matches!(
        FixedClock::new(rate, 0),
        Err(AppError::ZeroCatchUp)
    ));
}

#[test]
fn exact_step_gives_one_tick_and_no_remainder() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 8).expect("clock");
    let advance = clock.advance(STEP_60_HZ);
    assert_eq!(advance.ticks, 1);
    assert_eq!(advance.dropped_ticks, 0);
    assert_eq!(advance.remainder_nanos, 0);
}

#[test]
fn half_steps_accumulate_to_one_tick() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 8).expect("clock");
    let half = STEP_60_HZ / 2;
    let first = clock.advance(half);
    assert_eq!(first.ticks, 0);
    assert_eq!(first.remainder_nanos, half);
    let second = clock.advance(half);
    assert_eq!(second.ticks, 1);
    assert_eq!(second.remainder_nanos, 0);
}

#[test]
fn ten_steps_in_one_frame_give_ten_ticks() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 16).expect("clock");
    let advance = clock.advance(10 * STEP_60_HZ);
    assert_eq!(advance.ticks, 10);
    assert_eq!(advance.dropped_ticks, 0);
}

#[test]
fn catch_up_cap_runs_the_cap_and_drops_the_rest() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 5).expect("clock");
    let advance = clock.advance(12 * STEP_60_HZ);
    assert_eq!(advance.ticks, 5);
    assert_eq!(advance.dropped_ticks, 7);
    assert!(advance.remainder_nanos < STEP_60_HZ);
}

#[test]
fn alpha_is_remainder_over_step() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 8).expect("clock");
    let advance = clock.advance(STEP_60_HZ / 2);
    let alpha = advance.alpha(rate).expect("alpha in range");
    assert!((alpha - 0.5).abs() < 1e-6);
}

#[test]
fn alpha_stays_in_the_closed_unit_range_one_nanosecond_short_of_a_step() {
    let rate = TickRate::new(1).expect("valid rate");
    let step = rate.step_nanos();
    let mut clock = FixedClock::new(rate, 8).expect("clock");
    let advance = clock.advance(step - 1);
    assert_eq!(advance.ticks, 0);
    assert_eq!(advance.remainder_nanos, step - 1);
    let alpha = advance.alpha(rate).expect("alpha in range");
    assert!(
        (0.0..=1.0).contains(&alpha),
        "alpha {alpha} is outside [0, 1]"
    );
}

proptest! {
    /// Splitting the same total time into frames gives the same number of ticks as one frame,
    /// and that number is `total / step`.
    #[test]
    fn ticks_depend_only_on_total_time(
        frames in prop::collection::vec(0u64..100_000_000, 0..64),
    ) {
        let rate = TickRate::new(60).expect("valid rate");
        let step = rate.step_nanos();
        let total: u64 = frames.iter().sum();

        let mut split = FixedClock::new(rate, 10_000).expect("clock");
        let mut split_ticks = 0u64;
        for frame in &frames {
            let advance = split.advance(*frame);
            prop_assert_eq!(advance.dropped_ticks, 0);
            prop_assert!(advance.remainder_nanos < step);
            split_ticks += advance.ticks;
        }

        let mut single = FixedClock::new(rate, 10_000).expect("clock");
        let whole = single.advance(total);

        prop_assert_eq!(split_ticks, total / step);
        prop_assert_eq!(whole.ticks, total / step);
        prop_assert_eq!(whole.remainder_nanos, total % step);
    }

    /// Ticks run never exceed the cap, and run plus dropped equals the ticks owed.
    #[test]
    fn cap_accounting_is_exact(
        cap in 1u32..20,
        frame in 0u64..1_000_000_000,
    ) {
        let rate = TickRate::new(60).expect("valid rate");
        let step = rate.step_nanos();
        let mut clock = FixedClock::new(rate, cap).expect("clock");
        let advance = clock.advance(frame);
        let owed = frame / step;
        prop_assert!(advance.ticks <= u64::from(cap));
        prop_assert_eq!(advance.ticks + advance.dropped_ticks, owed);
        prop_assert!(advance.remainder_nanos < step);
    }
}
