//! Frame-time governor: picks the internal render scale from recent frame times so a slow device
//! holds its target frame rate.
//!
//! Pure arithmetic: no GPU, no clock reads, no threads. The caller passes each frame's time in
//! milliseconds. The last `window` frame times live in a buffer reserved once in [`Governor::new`],
//! so [`Governor::observe`] never allocates.
//!
//! Decisions are made once per full window of frames, on the window average. A window *misses*
//! when its average is over `target * LOWER_SLACK`; the slack exists because vsync reports frame
//! times of about 16.7 ms at 60 Hz, which is over a 16.6 ms target without any real miss.
//!
//! - A miss lowers the scale one step.
//! - A run of `probe_wait` windows that all meet the target lets the scale probe one step up.
//!   Under vsync a frame can never be shorter than the refresh interval, so a device that dipped
//!   cannot show that it has headroom; probing tests it directly.
//! - If the first window after a probe misses, the scale steps back down and the wait doubles
//!   (capped at `MAX_PROBE_WAIT`), so a device that cannot hold the higher step does not oscillate.
//!   A window that meets the target after a probe resets the wait to `PROBE_WINDOWS`.
//!
//! Invariants: the scale is always a whole number of `step`s and always within `[min, max]`; every
//! change clears the window, so a new decision needs a full window measured at the new scale; one
//! frame alone moves the window average only a little, so one spike changes nothing.

use std::collections::VecDeque;

use crate::RenderError;

/// Scale change per decision, so only a few distinct target sizes ever occur.
const DEFAULT_STEP: f32 = 0.05;
/// Frames averaged before a decision.
const DEFAULT_WINDOW: usize = 30;
/// A window misses only when its average is over this multiple of the target. Vsync at 60 Hz
/// reports about 16.7 ms against a 16.6 ms target, so a tight limit would lower the scale for
/// nothing.
const LOWER_SLACK: f32 = 1.10;
/// Consecutive windows that meet the target before the scale probes one step up.
const PROBE_WINDOWS: u32 = 4;
/// Longest wait between probes after repeated failed probes.
const MAX_PROBE_WAIT: u32 = 64;
/// Tolerance, in steps, for the f32 error of dividing a scale bound by the step.
const LEVEL_EPSILON: f32 = 1e-3;
/// Largest gap allowed between a start scale and its nearest whole step.
const STEP_TOLERANCE: f32 = 1e-5;
/// Lowest scale any governor may use (the renderer's own floor).
const SCALE_FLOOR: f32 = 0.4;
/// Highest scale any governor may use (native resolution).
const SCALE_CEILING: f32 = 1.0;

/// Validated settings for a [`Governor`]. The step and window take the defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GovernorConfig {
    target_ms: f32,
    min_scale: f32,
    max_scale: f32,
    step: f32,
    window: usize,
}

impl GovernorConfig {
    /// Settings that aim for `target_ms` per frame and keep the scale between `min_scale` and
    /// `max_scale`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidGovernor`] when `target_ms` is not finite and above zero,
    /// when the scales do not satisfy `0.4 <= min_scale <= max_scale <= 1.0`, or when no whole
    /// number of steps lies between them.
    pub fn new(target_ms: f32, min_scale: f32, max_scale: f32) -> Result<Self, RenderError> {
        if !(target_ms.is_finite() && target_ms > 0.0) {
            return Err(RenderError::InvalidGovernor(
                "target frame time must be finite and above zero",
            ));
        }
        if !(SCALE_FLOOR <= min_scale && min_scale <= max_scale && max_scale <= SCALE_CEILING) {
            return Err(RenderError::InvalidGovernor(
                "scales must satisfy 0.4 <= min <= max <= 1.0",
            ));
        }
        let config = Self {
            target_ms,
            min_scale,
            max_scale,
            step: DEFAULT_STEP,
            window: DEFAULT_WINDOW,
        };
        let (lowest, highest) = config.level_bounds();
        if lowest > highest {
            return Err(RenderError::InvalidGovernor(
                "no whole step lies between min and max scale",
            ));
        }
        Ok(config)
    }

    /// The whole-step levels that fit in `[min, max]`, as `(lowest, highest)` multiples of the step.
    /// The division error of f32 (for example `1.0 / 0.05` is just under 20) is absorbed by
    /// `LEVEL_EPSILON`, so a bound that is a whole step is included.
    fn level_bounds(&self) -> (f32, f32) {
        (
            (self.min_scale / self.step - LEVEL_EPSILON).ceil(),
            (self.max_scale / self.step + LEVEL_EPSILON).floor(),
        )
    }
}

/// Chooses the render scale from the frame times it has been shown.
#[derive(Debug)]
pub struct Governor {
    config: GovernorConfig,
    /// Current scale as a whole number of steps, kept as a float so no integer cast is needed.
    level: f32,
    /// The last `window` frame times in milliseconds, oldest first.
    frames: VecDeque<f32>,
    /// Frames observed since the last decision; a decision is made every `window` frames.
    since_decision: usize,
    /// Consecutive windows at this scale that met the target.
    met_windows: u32,
    /// Windows that must meet the target before the next probe up.
    probe_wait: u32,
    /// True from a probe step-up until its first window is judged.
    probing: bool,
}

impl Governor {
    /// A governor that starts at `start_scale`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidGovernor`] when `start_scale` is not a whole number of steps
    /// inside `[min, max]`. The renderer must already run at the start scale, so the governor never
    /// has to round it.
    pub fn new(config: GovernorConfig, start_scale: f32) -> Result<Self, RenderError> {
        let level = (start_scale / config.step).round();
        let (lowest, highest) = config.level_bounds();
        let on_step = (level * config.step - start_scale).abs() <= STEP_TOLERANCE;
        if !on_step || level < lowest || level > highest {
            return Err(RenderError::InvalidGovernor(
                "start scale must be a whole number of steps between min and max",
            ));
        }
        Ok(Self {
            config,
            level,
            frames: VecDeque::with_capacity(config.window),
            since_decision: 0,
            met_windows: 0,
            probe_wait: PROBE_WINDOWS,
            probing: false,
        })
    }

    /// Feeds one frame time in milliseconds. Returns `Some(new_scale)` only when the scale should
    /// change now; the caller then sets it on the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidFrameTime`] for a time that is NaN, infinite or negative.
    /// The governor keeps its state in that case.
    pub fn observe(&mut self, frame_ms: f32) -> Result<Option<f32>, RenderError> {
        if !(frame_ms.is_finite() && frame_ms >= 0.0) {
            return Err(RenderError::InvalidFrameTime(frame_ms));
        }
        let window = self.config.window;
        if self.frames.len() == window {
            self.frames.pop_front();
        }
        self.frames.push_back(frame_ms);
        self.since_decision = self.since_decision.saturating_add(1);
        if self.frames.len() < window || self.since_decision < window {
            return Ok(None);
        }
        self.since_decision = 0;

        let average = self.frames.iter().sum::<f32>() / self.frame_count();
        let (lowest, highest) = self.config.level_bounds();
        let meets = average <= self.config.target_ms * LOWER_SLACK;
        let next = match (self.probing, meets) {
            (true, false) => {
                // The probe step-up missed: step back and wait longer before the next probe.
                self.probing = false;
                self.probe_wait = self.probe_wait.saturating_mul(2).min(MAX_PROBE_WAIT);
                self.met_windows = 0;
                (self.level - 1.0).max(lowest)
            }
            (true, true) => {
                // The probe held: keep this scale and probe again after the shortest wait.
                self.probing = false;
                self.probe_wait = PROBE_WINDOWS;
                self.met_windows = 0;
                self.level
            }
            (false, false) => {
                self.met_windows = 0;
                (self.level - 1.0).max(lowest)
            }
            (false, true) => {
                self.met_windows = self.met_windows.saturating_add(1);
                if self.met_windows >= self.probe_wait && self.level < highest {
                    self.probing = true;
                    self.met_windows = 0;
                    (self.level + 1.0).min(highest)
                } else {
                    self.level
                }
            }
        };
        if next == self.level {
            return Ok(None);
        }
        self.level = next;
        self.frames.clear();
        Ok(Some(self.scale()))
    }

    /// The current render scale: a whole number of steps, within `[min, max]`.
    #[must_use]
    pub fn scale(&self) -> f32 {
        (self.level * self.config.step).clamp(self.config.min_scale, self.config.max_scale)
    }

    /// The number of frames in the window, as a float for averaging.
    fn frame_count(&self) -> f32 {
        // A window is at most a few hundred frames, far below the 2^24 where f32 stops being exact.
        self.frames.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Slow frames for the default 16.6 ms target: 30 ms each, a real miss.
    const SLOW_MS: f32 = 30.0;
    /// Fast frames: well under the target.
    const FAST_MS: f32 = 5.0;
    /// Frames that meet the target: the 60 Hz vsync interval.
    const VSYNC_MS: f32 = 16.7;

    fn config() -> Result<GovernorConfig, RenderError> {
        GovernorConfig::new(16.6, 0.5, 1.0)
    }

    /// Feeds `frames` copies of `ms` and returns every change the governor reported.
    fn feed(governor: &mut Governor, frames: usize, ms: f32) -> Result<Vec<f32>, RenderError> {
        let mut changes = Vec::new();
        for _ in 0..frames {
            if let Some(scale) = governor.observe(ms)? {
                changes.push(scale);
            }
        }
        Ok(changes)
    }

    /// Feeds one window of frame times per entry of `windows` (`window` frames each) and returns
    /// each change as `(1-based window number, new scale)`.
    fn run_script(
        governor: &mut Governor,
        windows: &[f32],
    ) -> Result<Vec<(usize, f32)>, RenderError> {
        let window = DEFAULT_WINDOW;
        let mut changes = Vec::new();
        for (index, ms) in windows.iter().enumerate() {
            for _ in 0..window {
                if let Some(scale) = governor.observe(*ms)? {
                    changes.push((index + 1, scale));
                }
            }
        }
        Ok(changes)
    }

    /// Asserts that a change was reported and is `expected`, within float rounding.
    fn assert_scale_change(change: Option<f32>, expected: f32) {
        assert!(
            change.is_some_and(|scale| (scale - expected).abs() < 1e-5),
            "expected a change to {expected}, got {change:?}"
        );
    }

    #[test]
    fn config_rejects_bad_target() {
        for target in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(GovernorConfig::new(target, 0.5, 1.0).is_err(), "{target}");
        }
    }

    #[test]
    fn config_rejects_scales_outside_the_renderer_range() {
        let cases = [
            (0.3, 1.0),
            (0.5, 1.1),
            (0.8, 0.6),
            (f32::NAN, 1.0),
            (0.5, f32::NAN),
        ];
        for (min, max) in cases {
            assert!(GovernorConfig::new(16.6, min, max).is_err(), "{min}..{max}");
        }
    }

    #[test]
    fn config_rejects_a_range_with_no_whole_step() {
        assert!(GovernorConfig::new(16.6, 0.41, 0.44).is_err());
    }

    #[test]
    fn config_accepts_the_floor_and_the_ceiling() -> Result<(), RenderError> {
        GovernorConfig::new(16.6, 0.4, 1.0)?;
        GovernorConfig::new(16.6, 1.0, 1.0)?;
        Ok(())
    }

    #[test]
    fn start_scale_must_be_a_step_inside_the_range() -> Result<(), RenderError> {
        let config = config()?;
        assert!(Governor::new(config, 0.87).is_err(), "off the step");
        assert!(Governor::new(config, 0.45).is_err(), "below the range");
        assert!(Governor::new(config, 1.05).is_err(), "above the range");
        assert!(Governor::new(config, f32::NAN).is_err());
        Governor::new(config, 0.85)?;
        Ok(())
    }

    #[test]
    fn vsync_frame_times_do_not_lower_the_scale() -> Result<(), RenderError> {
        // 16.7 ms is what a perfect 60 Hz display reports; it is over the 16.6 ms target but
        // inside the slack, so one full window must change nothing.
        let mut governor = Governor::new(config()?, 1.0)?;
        assert_eq!(feed(&mut governor, 30, VSYNC_MS)?, Vec::<f32>::new());
        assert!((governor.scale() - 1.0).abs() < 1e-5);
        Ok(())
    }

    #[test]
    fn a_real_miss_lowers_the_scale() -> Result<(), RenderError> {
        // 20 ms is over 16.6 * 1.10, so a full window of it is a miss.
        let mut governor = Governor::new(config()?, 1.0)?;
        let changes = feed(&mut governor, 30, 20.0)?;
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_scale_change(changes.first().copied(), 0.95);
        Ok(())
    }

    #[test]
    fn steady_slow_frames_lower_the_scale_step_by_step_to_the_minimum() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 1.0)?;
        let changes = feed(&mut governor, 30 * 12, SLOW_MS)?;
        // From 1.0 down to 0.5 is ten steps; the windows after that must not change anything.
        assert_eq!(changes.len(), 10, "{changes:?}");
        assert_scale_change(changes.first().copied(), 0.95);
        assert!((governor.scale() - 0.5).abs() < 1e-5);
        assert_eq!(feed(&mut governor, 30, SLOW_MS)?, Vec::<f32>::new());
        Ok(())
    }

    #[test]
    fn frames_that_meet_the_target_probe_back_up_to_the_maximum() -> Result<(), RenderError> {
        // Each step up takes PROBE_WINDOWS met windows, one probe window, and the probe's own
        // met window before the next wait starts: five windows per step, ten steps.
        let mut governor = Governor::new(config()?, 0.5)?;
        let changes = feed(&mut governor, 30 * 55, FAST_MS)?;
        assert_eq!(changes.len(), 10, "{changes:?}");
        assert!((governor.scale() - 1.0).abs() < 1e-5);
        assert_eq!(feed(&mut governor, 30 * 10, FAST_MS)?, Vec::<f32>::new());
        Ok(())
    }

    #[test]
    fn a_dip_recovers_to_the_maximum_under_vsync_frames() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 1.0)?;
        feed(&mut governor, 30 * 10, SLOW_MS)?;
        assert!((governor.scale() - 0.5).abs() < 1e-5);
        // Under vsync the frame time never drops below 16.7 ms, so recovery must come from probing.
        feed(&mut governor, 30 * 40, VSYNC_MS)?;
        assert!(governor.scale() < 1.0, "recovery is not instant");
        feed(&mut governor, 30 * 10, VSYNC_MS)?;
        assert!(
            (governor.scale() - 1.0).abs() < 1e-5,
            "{}",
            governor.scale()
        );
        Ok(())
    }

    #[test]
    fn met_windows_probe_up_after_the_wait_and_a_held_probe_keeps_the_scale()
    -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 0.8)?;
        // Windows 1 to 3 meet the target: no probe yet. Window 4 is the fourth in a row: probe.
        let changes = run_script(&mut governor, &[VSYNC_MS; 6])?;
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes.first().map(|c| c.0), Some(4));
        assert_scale_change(changes.first().map(|c| c.1), 0.85);
        Ok(())
    }

    #[test]
    fn a_probe_that_misses_steps_back_and_doubles_the_wait() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 0.8)?;
        let mut script = vec![VSYNC_MS; 4]; // w1-w4: met, probe up at w4
        script.push(SLOW_MS); // w5: the probe misses, step back down, wait becomes 8
        script.extend(vec![VSYNC_MS; 8]); // w6-w13: met, probe up at w13
        script.push(SLOW_MS); // w14: misses again, wait becomes 16
        script.extend(vec![VSYNC_MS; 16]); // w15-w30: met, probe up at w30
        script.push(VSYNC_MS); // w31: the probe holds, wait resets to 4

        let changes = run_script(&mut governor, &script)?;
        let observed: Vec<(usize, f32)> = changes
            .iter()
            .map(|(window, scale)| (*window, (scale * 100.0).round() / 100.0))
            .collect();
        assert_eq!(
            observed,
            vec![(4, 0.85), (5, 0.8), (13, 0.85), (14, 0.8), (30, 0.85)]
        );
        Ok(())
    }

    /// Feeds one full window of `ms` and returns the change it caused, if any.
    fn one_window(governor: &mut Governor, ms: f32) -> Result<Option<f32>, RenderError> {
        let mut change = None;
        for _ in 0..DEFAULT_WINDOW {
            if let Some(scale) = governor.observe(ms)? {
                change = Some(scale);
            }
        }
        Ok(change)
    }

    #[test]
    fn the_probe_wait_doubles_after_each_miss_and_stops_at_its_cap() -> Result<(), RenderError> {
        // Each cycle: met windows until the probe up (the wait), then one probe window that misses
        // and steps back down to 0.8.
        let mut governor = Governor::new(config()?, 0.8)?;
        let mut waits = Vec::new();
        for _ in 0..6 {
            let mut windows = 1;
            while one_window(&mut governor, VSYNC_MS)?.is_none() {
                windows += 1;
                assert!(windows <= 200, "no probe after {windows} windows");
            }
            waits.push(windows);
            assert_scale_change(one_window(&mut governor, SLOW_MS)?, 0.8);
        }
        assert_eq!(waits, vec![4, 8, 16, 32, 64, 64]);
        Ok(())
    }

    #[test]
    fn a_change_needs_a_fresh_full_window() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 1.0)?;
        assert_eq!(feed(&mut governor, 29, SLOW_MS)?, Vec::<f32>::new());
        assert_scale_change(governor.observe(SLOW_MS)?, 0.95);
        // The window was cleared, so the next change again needs 30 new frames.
        assert_eq!(feed(&mut governor, 29, SLOW_MS)?, Vec::<f32>::new());
        assert_scale_change(governor.observe(SLOW_MS)?, 0.9);
        Ok(())
    }

    #[test]
    fn one_spike_in_a_full_window_changes_nothing() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 0.8)?;
        // 29 frames at 16 ms and one at 30 ms average about 16.47 ms: inside the target, so the
        // window meets it and only counts toward a probe. A single spike cannot lower the scale.
        assert!(feed(&mut governor, 29, 16.0)?.is_empty());
        assert_eq!(governor.observe(30.0)?, None);
        assert!((governor.scale() - 0.8).abs() < 1e-5);
        Ok(())
    }

    #[test]
    fn invalid_frame_times_are_errors_and_leave_the_state_alone() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 1.0)?;
        feed(&mut governor, 29, SLOW_MS)?;
        for bad in [f32::NAN, f32::INFINITY, -0.5] {
            assert!(
                matches!(governor.observe(bad), Err(RenderError::InvalidFrameTime(_))),
                "{bad}"
            );
        }
        // The window still holds 29 frames, so the next valid frame completes it.
        assert_scale_change(governor.observe(SLOW_MS)?, 0.95);
        Ok(())
    }

    #[test]
    fn the_scale_is_always_a_whole_step_for_a_range_on_the_grid() -> Result<(), RenderError> {
        let mut governor = Governor::new(config()?, 0.75)?;
        for ms in [SLOW_MS, FAST_MS, SLOW_MS, VSYNC_MS] {
            feed(&mut governor, 30, ms)?;
            let steps = governor.scale() / DEFAULT_STEP;
            assert!((steps - steps.round()).abs() < 1e-3, "{}", governor.scale());
        }
        Ok(())
    }

    proptest! {
        /// For any sequence of finite frame times, the scale stays inside its range and on a whole step.
        #[test]
        fn scale_stays_in_range_on_the_step_grid(
            start in 0u8..=10,
            frames in prop::collection::vec(0.0f32..500.0, 0..1200),
        ) {
            let config = GovernorConfig::new(16.6, 0.5, 1.0)
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            let start_scale = 0.5 + f32::from(start) * DEFAULT_STEP;
            let mut governor = Governor::new(config, start_scale)
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            for ms in frames {
                if let Some(scale) = governor.observe(ms).map_err(|e| TestCaseError::fail(e.to_string()))? {
                    prop_assert_eq!(scale, governor.scale());
                }
                let scale = governor.scale();
                prop_assert!((0.5..=1.0).contains(&scale), "{scale}");
                let steps = scale / DEFAULT_STEP;
                prop_assert!((steps - steps.round()).abs() < 1e-3, "{scale}");
            }
        }
    }
}
