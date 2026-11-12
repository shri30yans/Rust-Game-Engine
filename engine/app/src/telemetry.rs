//! Frame telemetry: a ring of the last [`WINDOW_FRAMES`] frame times and the simulation cost
//! of each frame, reduced to the numbers the overlay shows. This is pure arithmetic: the caller
//! passes each frame's wall time and tick cost in milliseconds, so the module reads no clock.
//!
//! The refresh estimate is derived, never queried. Under vsync a frame lasts one refresh interval,
//! so the median frame time names the display rate, rounded to the nearest common rate.
//!
//! Invariants: stored frame values are finite and not negative; `record` and `snapshot` allocate
//! nothing after `new`; an empty window has zero timing values and no refresh estimate.

use crate::error::AppError;

/// Frames kept in the window. 240 frames is 2 s at 120 Hz.
pub const WINDOW_FRAMES: usize = 240;

/// The refresh rates the estimate can name, in Hz.
pub const REFRESH_RATES_HZ: [u16; 7] = [30, 45, 60, 72, 90, 120, 144];

/// The numbers one snapshot of the window reports. Plain values, copied out of the telemetry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameReport {
    /// Frames per second over the window: 1000 divided by the mean frame time. Zero when the
    /// window is empty.
    pub fps: f32,
    /// Mean wall time per frame, in milliseconds.
    pub frame_ms_avg: f32,
    /// The 99th-percentile frame time (nearest rank), in milliseconds: the slowest 1 % of frames
    /// are at least this long.
    pub frame_ms_p99: f32,
    /// The slowest frame in the window, in milliseconds.
    pub frame_ms_max: f32,
    /// The display rate: the median frame time, as a rate, rounded to the nearest of
    /// [`REFRESH_RATES_HZ`]. `None` when the window is empty.
    pub refresh_hz_estimate: Option<u16>,
    /// Mean simulation cost per frame (ticks and extract), in milliseconds.
    pub sim_ms_avg: f32,
    /// Presentation errors reported since this client was built.
    pub presentation_errors: u64,
}

/// The frame-time window. Build it once with [`Telemetry::new`], which reserves every buffer.
#[derive(Debug)]
pub struct Telemetry {
    /// Wall time of each frame in the window, in milliseconds. Slots past `filled` are unused.
    frame_ms: Vec<f32>,
    /// Simulation cost of each frame in the window, in milliseconds.
    sim_ms: Vec<f32>,
    /// Selection scratch, reused by every snapshot.
    scratch: Vec<f32>,
    /// The slot the next record writes.
    next: usize,
    /// Valid samples, at most [`WINDOW_FRAMES`].
    filled: usize,
    /// Recoverable presentation failures reported by the client.
    presentation_errors: u64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    /// An empty window with every buffer reserved.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame_ms: vec![0.0; WINDOW_FRAMES],
            sim_ms: vec![0.0; WINDOW_FRAMES],
            scratch: Vec::with_capacity(WINDOW_FRAMES),
            next: 0,
            filled: 0,
            presentation_errors: 0,
        }
    }

    /// Records one recoverable presentation error for the telemetry overlay.
    pub(crate) fn record_presentation_error(&mut self) {
        self.presentation_errors = self.presentation_errors.saturating_add(1);
    }

    /// Adds one frame: its wall time and its simulation cost, both in milliseconds. Once the
    /// window is full, the oldest frame is overwritten.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidFrameTime`] when either value is NaN, infinite or negative. The
    /// window is left unchanged.
    pub fn record(&mut self, frame_ms: f32, sim_ms: f32) -> Result<(), AppError> {
        check_ms(frame_ms)?;
        check_ms(sim_ms)?;
        *slot(&mut self.frame_ms, self.next)? = frame_ms;
        *slot(&mut self.sim_ms, self.next)? = sim_ms;
        self.next = (self.next + 1) % WINDOW_FRAMES;
        self.filled = (self.filled + 1).min(WINDOW_FRAMES);
        Ok(())
    }

    /// Reduces the window to its report. The selection reorders an internal scratch buffer, which
    /// is why this takes `&mut self`; no allocation happens.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::TelemetryIndex`] only if an internal index is out of range, which means
    /// a bug.
    pub fn snapshot(&mut self) -> Result<FrameReport, AppError> {
        let n = self.filled;
        if n == 0 {
            return Ok(FrameReport {
                presentation_errors: self.presentation_errors,
                ..FrameReport::default()
            });
        }
        let frames = self.frame_ms.get(..n).ok_or(AppError::TelemetryIndex(n))?;
        let sims = self.sim_ms.get(..n).ok_or(AppError::TelemetryIndex(n))?;
        let frame_ms_avg = mean(frames);
        let frame_ms_max = frames.iter().copied().fold(0.0_f32, f32::max);
        let sim_ms_avg = mean(sims);
        let frame_ms_p99 = select_rank(&mut self.scratch, frames, 99)?;
        let median = select_rank(&mut self.scratch, frames, 50)?;
        let fps = if frame_ms_avg > 0.0 {
            1000.0 / frame_ms_avg
        } else {
            0.0
        };
        let refresh_hz_estimate = if median > 0.0 {
            nearest_refresh(1000.0 / median)
        } else {
            None
        };
        Ok(FrameReport {
            fps,
            frame_ms_avg,
            frame_ms_p99,
            frame_ms_max,
            refresh_hz_estimate,
            sim_ms_avg,
            presentation_errors: self.presentation_errors,
        })
    }
}

/// Rejects a value that is NaN, infinite or negative.
fn check_ms(value: f32) -> Result<(), AppError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(AppError::InvalidFrameTime(value))
    }
}

/// The slot `index` of a ring buffer, or an error when the index is out of range.
fn slot(values: &mut [f32], index: usize) -> Result<&mut f32, AppError> {
    values.get_mut(index).ok_or(AppError::TelemetryIndex(index))
}

/// Mean of a non-empty slice.
fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

/// The nearest-rank value at `percent` of `source`: copies it into `scratch` and selects the rank.
/// Nearest rank: the value at 1-based rank ceil(percent * n / 100).
fn select_rank(scratch: &mut Vec<f32>, source: &[f32], percent: usize) -> Result<f32, AppError> {
    scratch.clear();
    scratch.extend_from_slice(source);
    let index = (scratch.len() * percent).div_ceil(100).saturating_sub(1);
    nth_value(scratch, index)
}

/// The value that would sit at `index` if `values` were sorted. Reorders `values`.
fn nth_value(values: &mut [f32], index: usize) -> Result<f32, AppError> {
    if index >= values.len() {
        return Err(AppError::TelemetryIndex(index));
    }
    let (_, value, _) = values.select_nth_unstable_by(index, f32::total_cmp);
    Ok(*value)
}

/// The common refresh rate nearest to `hz`, or `None` when the list is somehow empty.
fn nearest_refresh(hz: f32) -> Option<u16> {
    REFRESH_RATES_HZ.iter().copied().min_by(|a, b| {
        (f32::from(*a) - hz)
            .abs()
            .total_cmp(&(f32::from(*b) - hz).abs())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A telemetry with `frames` recorded at `frame_ms` each and no simulation cost.
    fn filled(frame_ms: f32, frames: usize) -> Result<Telemetry, AppError> {
        let mut telemetry = Telemetry::new();
        for _ in 0..frames {
            telemetry.record(frame_ms, 0.0)?;
        }
        Ok(telemetry)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn an_empty_window_reports_zeros_and_no_refresh_rate() -> Result<(), AppError> {
        let report = Telemetry::new().snapshot()?;
        assert_eq!(report, FrameReport::default());
        assert_eq!(report.refresh_hz_estimate, None);
        Ok(())
    }

    #[test]
    fn a_120_hz_sequence_reports_120_fps_and_120_hz() -> Result<(), AppError> {
        let frame = 1000.0 / 120.0;
        let report = filled(frame, 300)?.snapshot()?;
        assert!(close(report.fps, 120.0), "fps {}", report.fps);
        assert!(close(report.frame_ms_avg, frame));
        assert!(close(report.frame_ms_p99, frame));
        assert!(close(report.frame_ms_max, frame));
        assert_eq!(report.refresh_hz_estimate, Some(120));
        Ok(())
    }

    #[test]
    fn a_60_hz_sequence_reports_60_hz() -> Result<(), AppError> {
        let report = filled(1000.0 / 60.0, 10)?.snapshot()?;
        assert!(close(report.fps, 60.0));
        assert_eq!(report.refresh_hz_estimate, Some(60));
        Ok(())
    }

    #[test]
    fn presentation_errors_are_visible_even_before_the_first_frame() -> Result<(), AppError> {
        let mut telemetry = Telemetry::new();
        telemetry.record_presentation_error();
        let report = telemetry.snapshot()?;
        assert_eq!(report.presentation_errors, 1);
        assert_eq!(report.fps, 0.0);
        Ok(())
    }

    #[test]
    fn the_refresh_estimate_rounds_to_the_nearest_common_rate() -> Result<(), AppError> {
        // 1000 / 11.2 ms = 89.3 Hz, nearest to 90; 1000 / 13.3 ms = 75.2 Hz, nearest to 72.
        let near_90 = filled(11.2, 5)?.snapshot()?;
        assert_eq!(near_90.refresh_hz_estimate, Some(90));
        let near_72 = filled(13.3, 5)?.snapshot()?;
        assert_eq!(near_72.refresh_hz_estimate, Some(72));
        Ok(())
    }

    #[test]
    fn the_p99_is_the_nearest_rank_value() -> Result<(), AppError> {
        // Frame times 1..=100 ms: nearest rank of 99 % is rank 99, which is 99 ms.
        let mut telemetry = Telemetry::new();
        for ms in 1..=100 {
            telemetry.record(ms as f32, 0.0)?;
        }
        let report = telemetry.snapshot()?;
        assert!(
            close(report.frame_ms_p99, 99.0),
            "p99 {}",
            report.frame_ms_p99
        );
        assert!(close(report.frame_ms_max, 100.0));
        assert!(close(report.frame_ms_avg, 50.5));
        Ok(())
    }

    #[test]
    fn a_single_frame_is_its_own_percentile_and_maximum() -> Result<(), AppError> {
        let report = filled(7.5, 1)?.snapshot()?;
        assert!(close(report.frame_ms_p99, 7.5));
        assert!(close(report.frame_ms_max, 7.5));
        Ok(())
    }

    #[test]
    fn one_slow_frame_in_a_hundred_shows_in_the_p99_and_the_max() -> Result<(), AppError> {
        let mut telemetry = filled(8.0, 99)?;
        telemetry.record(40.0, 0.0)?;
        let report = telemetry.snapshot()?;
        // The 99th of 100 frames is the last fast one; the slow frame is the max.
        assert!(close(report.frame_ms_p99, 8.0));
        assert!(close(report.frame_ms_max, 40.0));
        Ok(())
    }

    #[test]
    fn the_window_keeps_only_the_newest_frames() -> Result<(), AppError> {
        // 240 slow frames, then 240 fast ones: the slow ones have left the window.
        let mut telemetry = filled(50.0, WINDOW_FRAMES)?;
        for _ in 0..WINDOW_FRAMES {
            telemetry.record(10.0, 0.0)?;
        }
        let report = telemetry.snapshot()?;
        assert!(close(report.frame_ms_max, 10.0));
        assert!(close(report.fps, 100.0));
        Ok(())
    }

    #[test]
    fn the_simulation_cost_is_averaged_over_the_same_window() -> Result<(), AppError> {
        let mut telemetry = Telemetry::new();
        telemetry.record(8.0, 0.2)?;
        telemetry.record(8.0, 0.6)?;
        let report = telemetry.snapshot()?;
        assert!(close(report.sim_ms_avg, 0.4), "sim {}", report.sim_ms_avg);
        Ok(())
    }

    #[test]
    fn nan_infinite_and_negative_times_are_errors_and_change_nothing() -> Result<(), AppError> {
        let mut telemetry = filled(8.0, 3)?;
        let before = telemetry.snapshot()?;
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(matches!(
                telemetry.record(bad, 0.0),
                Err(AppError::InvalidFrameTime(_))
            ));
            assert!(matches!(
                telemetry.record(8.0, bad),
                Err(AppError::InvalidFrameTime(_))
            ));
        }
        assert_eq!(telemetry.snapshot()?, before);
        Ok(())
    }

    #[test]
    fn a_zero_frame_time_is_accepted_and_reports_no_refresh_rate() -> Result<(), AppError> {
        // A zero frame (the first frame after start-up) must not divide by zero.
        let report = filled(0.0, 4)?.snapshot()?;
        assert!(close(report.fps, 0.0));
        assert_eq!(report.refresh_hz_estimate, None);
        Ok(())
    }
}
