//! Property tests of the frame telemetry: whatever finite, non-negative frame times go in,
//! every reported number is finite, the maximum bounds the 99th percentile, and the fps follows the
//! mean frame time.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{FrameReport, Telemetry, WINDOW_FRAMES};
use proptest::prelude::*;

proptest! {
    #[test]
    fn reports_are_finite_and_ordered(
        frames in proptest::collection::vec((0.0_f32..200.0, 0.0_f32..20.0), 1..600),
    ) {
        let mut telemetry = Telemetry::new();
        for (frame_ms, sim_ms) in &frames {
            telemetry.record(*frame_ms, *sim_ms).expect("finite and non-negative");
        }
        let report: FrameReport = telemetry.snapshot().expect("snapshot");
        for value in [report.fps, report.frame_ms_avg, report.frame_ms_p99, report.frame_ms_max, report.sim_ms_avg] {
            prop_assert!(value.is_finite(), "{report:?}");
            prop_assert!(value >= 0.0, "{report:?}");
        }
        prop_assert!(report.frame_ms_max >= report.frame_ms_p99, "{report:?}");
        // The window holds the newest WINDOW_FRAMES frames; the maximum is over exactly those.
        let window_max = frames
            .iter()
            .rev()
            .take(WINDOW_FRAMES)
            .map(|(f, _)| *f)
            .fold(0.0_f32, f32::max);
        prop_assert_eq!(report.frame_ms_max, window_max);
        if report.frame_ms_avg > 0.0 {
            prop_assert!((report.fps * report.frame_ms_avg - 1000.0).abs() < 0.5, "{report:?}");
        }
    }

    #[test]
    fn a_steady_rate_reports_its_own_refresh_rate(rate in prop::sample::select(vec![30_u16, 45, 60, 72, 90, 120, 144])) {
        let mut telemetry = Telemetry::new();
        let frame_ms = 1000.0 / f32::from(rate);
        for _ in 0..WINDOW_FRAMES {
            telemetry.record(frame_ms, 0.1).expect("valid");
        }
        let report = telemetry.snapshot().expect("snapshot");
        prop_assert_eq!(report.refresh_hz_estimate, Some(rate));
    }
}
