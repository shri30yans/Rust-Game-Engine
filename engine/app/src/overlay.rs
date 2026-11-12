//! The frame-telemetry overlay: a compact panel of the numbers in a [`FrameReport`], drawn by
//! any game's presenter. Drawing reads plain values and changes no state.
//!
//! The first two lines take the severity colour: amber when the 99th-percentile frame time is
//! longer than the display interval, red when it is at least twice that. The interval is
//! 1000 divided by the estimated refresh rate, or by 60 when the rate is unknown.
//!
//! Invariants: every position scales with the target height, so the panel keeps its place at any
//! size; the panel sits below a top inset kept for the status bar and the notch.

use glam::Vec2;
use platform::{PresentationBinding, Rect, TouchRegion, TouchRegionId, TouchShape, Viewport};
use render::{Align, FontId, GpuTimes, Ui, ViewMode, rgba};

use crate::telemetry::FrameReport;

/// The id of the telemetry touch region. Games number their own regions from 0 upward and must
/// not use this id; the engine reserves the top value.
pub const TELEMETRY_TOUCH_ID: TouchRegionId = TouchRegionId(u8::MAX);

/// The height the panel is designed at. One logical unit is one pixel at this height.
const LOGICAL_HEIGHT: f32 = 720.0;
/// The display rate assumed when the window has no refresh estimate.
const ASSUMED_HZ: u16 = 60;
/// Space above the panel, in logical units, kept for the status bar and the notch.
const TOP_INSET: f32 = 48.0;
/// Distance of the panel from the left edge, in logical units.
const LEFT: f32 = 24.0;
/// Font size of every line, in logical units.
const FONT_PX: f32 = 20.0;
/// Vertical distance between two lines, in logical units.
const LINE_STEP: f32 = 24.0;
/// Padding inside the panel, in logical units.
const PAD: f32 = 10.0;
/// Corner radius of the panel, in logical units.
const RADIUS: f32 = 6.0;

/// How the frame times compare with the display interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The 99th-percentile frame fits in one display interval.
    Smooth,
    /// The 99th-percentile frame is longer than one display interval.
    Slow,
    /// The 99th-percentile frame is at least two display intervals.
    Stutter,
}

impl Severity {
    /// The text colour of this severity (linear RGBA).
    #[must_use]
    pub fn colour(self) -> [f32; 4] {
        match self {
            Self::Smooth => rgba(0xffffff, 0.95),
            Self::Slow => rgba(0xffb020, 1.0),
            Self::Stutter => rgba(0xff4040, 1.0),
        }
    }
}

/// The display interval in milliseconds that the severity compares against.
#[must_use]
pub fn display_interval_ms(report: &FrameReport) -> f32 {
    1000.0 / f32::from(report.refresh_hz_estimate.unwrap_or(ASSUMED_HZ))
}

/// How the 99th-percentile frame time compares with the display interval.
#[must_use]
pub fn severity(report: &FrameReport) -> Severity {
    let interval = display_interval_ms(report);
    if report.frame_ms_p99 >= 2.0 * interval {
        Severity::Stutter
    } else if report.frame_ms_p99 > interval {
        Severity::Slow
    } else {
        Severity::Smooth
    }
}

/// The panel's text lines. The first two take the severity colour; the rest are plain.
fn present_line(binding: PresentationBinding, measured: Option<u16>) -> String {
    let measured = measured
        .map(|hz| format!("measured ~{hz} Hz"))
        .unwrap_or_else(|| "measured rate unknown".to_string());
    match binding.intended_present_hz {
        Some(target) if binding.declared => {
            format!("present {target:.0} Hz declared  {measured}")
        }
        Some(target) => format!("present {target:.0} Hz not accepted  {measured}"),
        None => measured,
    }
}

fn lines(
    report: &FrameReport,
    render_scale: f32,
    gpu: Option<&GpuTimes>,
    view_mode: ViewMode,
    presentation: PresentationBinding,
) -> Vec<String> {
    let rate = present_line(presentation, report.refresh_hz_estimate);
    let mut lines = vec![
        format!("{:.0} fps  {:.1} ms", report.fps, report.frame_ms_avg),
        format!(
            "1% low {:.1} ms  max {:.1} ms",
            report.frame_ms_p99, report.frame_ms_max
        ),
        format!("{rate}  scale {:.0}%", render_scale * 100.0),
        format!("sim {:.1} ms", report.sim_ms_avg),
        format!("presentation errors {}", report.presentation_errors),
        format!("view {}", view_mode.label()),
    ];
    if let Some(gpu) = gpu {
        lines.push(format!("gpu {:.1} ms", gpu.frame));
    }
    lines
}

/// Draws the telemetry panel at the top left of `ui`, in `font`.
///
/// `render_scale` is the internal resolution scale in effect, `gpu` the latest GPU timing when the
/// adapter reports one.
pub fn draw_overlay(
    ui: &mut Ui,
    font: FontId,
    report: &FrameReport,
    render_scale: f32,
    gpu: Option<&GpuTimes>,
    view_mode: ViewMode,
    presentation: PresentationBinding,
) -> Option<TouchRegion> {
    let unit = ui.size.y / LOGICAL_HEIGHT;
    let origin = Vec2::new(LEFT * unit, TOP_INSET * unit);
    let pad = PAD * unit;
    let px = FONT_PX * unit;
    let severity_colour = severity(report).colour();
    let text = lines(report, render_scale, gpu, view_mode, presentation);
    let width = text
        .iter()
        .map(|line| ui.measure(font, px, line))
        .fold(0.0_f32, f32::max)
        + 2.0 * pad;
    let height = text.len() as f32 * LINE_STEP * unit + 2.0 * pad;
    ui.rect(
        origin.x,
        origin.y,
        width,
        height,
        RADIUS * unit,
        rgba(0x000000, 0.45),
    );
    let mut y = origin.y + pad;
    for (index, line) in text.iter().enumerate() {
        let colour = if index < 2 {
            severity_colour
        } else {
            rgba(0xffffff, 0.8)
        };
        ui.text(font, origin.x + pad, y, px, colour, Align::Left, line);
        y += LINE_STEP * unit;
    }
    Viewport::new(ui.size.x, ui.size.y)?
        .clip(Rect {
            x: origin.x,
            y: origin.y,
            w: width,
            h: height,
        })
        .map(|rect| TouchRegion {
            id: TELEMETRY_TOUCH_ID,
            rect,
            shape: TouchShape::Rect,
            drag_look: false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    /// The Roboto Medium the banner and the games use.
    const FONT: &[u8] = include_bytes!("../../render/fonts/Roboto-Medium.ttf");

    /// A report with a 99th-percentile frame of `p99_ms` on a display of `hz` (or unknown).
    fn report(p99_ms: f32, hz: Option<u16>) -> FrameReport {
        FrameReport {
            fps: 60.0,
            frame_ms_avg: 16.0,
            frame_ms_p99: p99_ms,
            frame_ms_max: p99_ms,
            refresh_hz_estimate: hz,
            sim_ms_avg: 0.4,
            presentation_errors: 0,
        }
    }

    #[test]
    fn severity_turns_amber_past_one_interval_and_red_at_two() {
        // 60 Hz: interval 16.67 ms.
        assert_eq!(severity(&report(16.0, Some(60))), Severity::Smooth);
        assert_eq!(severity(&report(17.0, Some(60))), Severity::Slow);
        assert_eq!(severity(&report(33.0, Some(60))), Severity::Slow);
        assert_eq!(severity(&report(33.4, Some(60))), Severity::Stutter);
    }

    #[test]
    fn severity_follows_the_display_rate_not_a_fixed_sixty() {
        // 120 Hz: interval 8.33 ms, so 9 ms is already slow.
        assert_eq!(severity(&report(9.0, Some(120))), Severity::Slow);
        assert_eq!(severity(&report(9.0, Some(60))), Severity::Smooth);
    }

    #[test]
    fn an_unknown_rate_is_judged_against_sixty_hertz() {
        assert_eq!(display_interval_ms(&report(0.0, None)), 1000.0 / 60.0);
        assert_eq!(severity(&report(17.0, None)), Severity::Slow);
    }

    /// Counts the vertices in `ui` drawn in exactly `colour`.
    fn count_colour(ui: &Ui, colour: [f32; 4]) -> usize {
        ui.verts.iter().filter(|v| v.color == colour).count()
    }

    #[test]
    fn the_panel_draws_its_text_in_the_colour_of_its_severity() -> Result<(), AppError> {
        let mut ui = Ui::new();
        ui.size = Vec2::new(1280.0, 720.0);
        let font = ui.atlas.add_font(FONT)?;
        let cases = [
            (report(16.0, Some(60)), Severity::Smooth),
            (report(20.0, Some(60)), Severity::Slow),
            (report(40.0, Some(60)), Severity::Stutter),
        ];
        for (sample, expected) in cases {
            ui.clear();
            draw_overlay(
                &mut ui,
                font,
                &sample,
                1.0,
                None,
                ViewMode::Lit,
                PresentationBinding::default(),
            );
            assert!(
                count_colour(&ui, expected.colour()) > 0,
                "no text in the {expected:?} colour"
            );
            for other in [Severity::Smooth, Severity::Slow, Severity::Stutter] {
                if other != expected {
                    assert_eq!(
                        count_colour(&ui, other.colour()),
                        0,
                        "{expected:?} text also drawn in the {other:?} colour"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn the_telemetry_region_sits_over_the_top_left_corner_inside_the_screen() -> Result<(), AppError>
    {
        for (width, height) in [(1600.0_f32, 900.0_f32), (2400.0, 1080.0)] {
            let mut ui = Ui::new();
            ui.size = Vec2::new(width, height);
            let font = ui.atlas.add_font(FONT)?;
            let region = draw_overlay(
                &mut ui,
                font,
                &report(16.0, Some(60)),
                1.0,
                None,
                ViewMode::Lit,
                PresentationBinding::default(),
            )
            .ok_or(AppError::ClientNotReady)?;
            let rect = region.rect.rect();
            assert!(rect.x >= 0.0 && rect.y >= 0.0);
            assert!(rect.x + rect.w <= width && rect.y + rect.h <= height);
            assert!(rect.x < width * 0.5 && rect.y < height * 0.5);
            assert_eq!(region.id, TELEMETRY_TOUCH_ID);
        }
        Ok(())
    }

    #[test]
    fn telemetry_lists_the_selected_view_mode() {
        assert!(
            lines(
                &report(16.0, Some(60)),
                1.0,
                None,
                ViewMode::AmbientOcclusion,
                PresentationBinding::default(),
            )
            .iter()
            .any(|line| line == "view Ambient occlusion")
        );
    }

    #[test]
    fn the_gpu_line_is_drawn_only_when_the_gpu_reports_times() -> Result<(), AppError> {
        let mut ui = Ui::new();
        ui.size = Vec2::new(1280.0, 720.0);
        let font = ui.atlas.add_font(FONT)?;
        let sample = report(16.0, Some(60));
        draw_overlay(
            &mut ui,
            font,
            &sample,
            1.0,
            None,
            ViewMode::Lit,
            PresentationBinding::default(),
        );
        let without = ui.verts.len();
        ui.clear();
        let gpu = GpuTimes {
            frame: 4.2,
            ..GpuTimes::default()
        };
        draw_overlay(
            &mut ui,
            font,
            &sample,
            1.0,
            Some(&gpu),
            ViewMode::Lit,
            PresentationBinding::default(),
        );
        assert!(ui.verts.len() > without, "the gpu line adds glyphs");
        Ok(())
    }
}
