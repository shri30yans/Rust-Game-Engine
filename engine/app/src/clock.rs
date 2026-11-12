//! Deterministic fixed-step clock.
//!
//! The clock keeps a nanosecond accumulator in integer arithmetic. The caller
//! passes the elapsed nanoseconds of each rendered frame; this module never
//! reads the wall clock. Invariants: the step is never zero (the rate is
//! 1..=1000 Hz, so the step is at least one million nanoseconds), and after
//! every call the accumulator holds less than one step.

use std::num::{NonZeroU16, NonZeroU32};

use crate::error::AppError;

/// Nanoseconds in one second.
const NANOS_PER_SECOND: u64 = 1_000_000_000;
/// Highest simulation rate the clock accepts.
const MAX_TICK_RATE_HZ: u16 = 1000;

/// Simulation ticks per second, 1..=1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickRate(NonZeroU16);

impl TickRate {
    /// Creates a tick rate.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidTickRate`] when `hz` is 0 or above 1000.
    pub fn new(hz: u16) -> Result<Self, AppError> {
        match NonZeroU16::new(hz) {
            Some(rate) if rate.get() <= MAX_TICK_RATE_HZ => Ok(Self(rate)),
            _ => Err(AppError::InvalidTickRate(hz)),
        }
    }

    /// Ticks per second.
    #[must_use]
    pub const fn hz(self) -> u16 {
        self.0.get()
    }

    /// Length of one tick in nanoseconds, rounded down.
    #[must_use]
    pub fn step_nanos(self) -> u64 {
        NANOS_PER_SECOND / u64::from(self.0.get())
    }

    /// Length of one tick in seconds, as the `dt` handed to systems.
    #[must_use]
    pub fn dt_seconds(self) -> f32 {
        1.0 / f32::from(self.0.get())
    }
}

/// What [`FixedClock::advance`] decided for one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advance {
    /// Whole ticks the caller must run now (at most the catch-up cap).
    pub ticks: u64,
    /// Owed ticks discarded because they exceeded the catch-up cap.
    pub dropped_ticks: u64,
    /// Nanoseconds left over after this frame; always less than one step.
    pub remainder_nanos: u64,
}

impl Advance {
    /// How far the next tick is, as `remainder / step`, for render interpolation.
    ///
    /// The result is in `[0, 1]`. A remainder just below the step can round up to exactly `1.0`
    /// in f32, so callers must accept both ends.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ClockRange`] if the remainder or the step does not fit in `u32`.
    /// Neither happens for rates within 1..=1000 Hz, because the step is at most 10^9 ns.
    pub fn alpha(&self, rate: TickRate) -> Result<f32, AppError> {
        let remainder = u32::try_from(self.remainder_nanos).map_err(|_| AppError::ClockRange)?;
        let step = u32::try_from(rate.step_nanos()).map_err(|_| AppError::ClockRange)?;
        // Rounding to f32 is monotonic, so remainder < step gives a rounded remainder at most the
        // rounded step, and IEEE division of a value by a larger-or-equal value is at most 1.
        // Exactly 1.0 can result from rounding alone.
        Ok(remainder as f32 / step as f32)
    }
}

/// Turns elapsed frame time into a whole number of fixed ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedClock {
    rate: TickRate,
    max_catch_up: NonZeroU32,
    accumulator_nanos: u64,
}

impl FixedClock {
    /// Creates a clock at rate `rate` that runs at most `max_catch_up_ticks` ticks per call.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ZeroCatchUp`] when `max_catch_up_ticks` is 0.
    pub fn new(rate: TickRate, max_catch_up_ticks: u32) -> Result<Self, AppError> {
        let max_catch_up = NonZeroU32::new(max_catch_up_ticks).ok_or(AppError::ZeroCatchUp)?;
        Ok(Self {
            rate,
            max_catch_up,
            accumulator_nanos: 0,
        })
    }

    /// Adds `frame_nanos` and returns how many whole ticks to run.
    ///
    /// If more than the catch-up cap is owed, the cap is run, the rest is reported in
    /// `dropped_ticks`, and only the sub-tick remainder is kept. The accumulator saturates at
    /// `u64::MAX` nanoseconds instead of wrapping.
    pub fn advance(&mut self, frame_nanos: u64) -> Advance {
        let step = self.rate.step_nanos();
        self.accumulator_nanos = self.accumulator_nanos.saturating_add(frame_nanos);
        // `step` is at least 10^6 ns (rate is at most 1000 Hz), so these divisions cannot fail.
        let owed = self.accumulator_nanos / step;
        let remainder_nanos = self.accumulator_nanos % step;
        let ticks = owed.min(u64::from(self.max_catch_up.get()));
        self.accumulator_nanos = remainder_nanos;
        Advance {
            ticks,
            dropped_ticks: owed - ticks,
            remainder_nanos,
        }
    }
}
