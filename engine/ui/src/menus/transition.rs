//! Timed screen transitions and easing.

use crate::stack::Layer;

use super::input::{REDUCED_MOTION_MS, Transition};

use super::runtime::OpenScreen;

pub(super) fn update_transition(
    screen: &mut OpenScreen,
    now: u64,
    reduce: bool,
    exit_ms: u32,
    slide: f32,
) {
    if screen.covered {
        let duration = if reduce { REDUCED_MOTION_MS } else { exit_ms };
        let t = progress(now.saturating_sub(screen.cover_started_ms), duration);
        let eased = if reduce {
            t
        } else {
            cubic_bezier(t, screen.curve)
        };
        screen.transition = Transition {
            opacity: 1.0 - eased,
            offset_x: 0.0,
            scale: 1.0,
        };
        return;
    }
    if screen.resuming {
        let duration = if reduce { REDUCED_MOTION_MS } else { exit_ms };
        let t = progress(now.saturating_sub(screen.resume_started_ms), duration);
        let eased = if reduce {
            t
        } else {
            cubic_bezier(t, screen.curve)
        };
        screen.transition = Transition {
            opacity: eased,
            offset_x: 0.0,
            scale: 1.0,
        };
        if t >= 1.0 {
            screen.resuming = false;
        }
        return;
    }
    let elapsed = if screen.exiting {
        now.saturating_sub(screen.exit_started_ms)
    } else {
        now.saturating_sub(screen.started_ms)
    };
    let duration = if screen.exiting {
        if reduce { REDUCED_MOTION_MS } else { exit_ms }
    } else {
        screen.duration_ms
    };
    let t = progress(elapsed, duration);
    let eased = if reduce {
        t
    } else {
        cubic_bezier(t, screen.curve)
    };
    screen.transition = if screen.exiting {
        Transition {
            opacity: 1.0 - eased,
            offset_x: 0.0,
            scale: 1.0,
        }
    } else {
        Transition {
            opacity: eased,
            offset_x: slide * (1.0 - eased),
            scale: if screen.layer == Layer::Modal {
                0.96 + 0.04 * eased
            } else {
                1.0
            },
        }
    };
}

pub(super) fn layer_order(layer: Layer) -> u8 {
    match layer {
        Layer::Game => 0,
        Layer::GameMenu => 1,
        Layer::Menu => 2,
        Layer::Modal => 3,
        Layer::Toast => 4,
    }
}

pub(super) fn progress(elapsed: u64, duration: u32) -> f32 {
    if duration == 0 {
        return 1.0;
    }
    let denominator = f32::from(u16::try_from(duration).unwrap_or(u16::MAX));
    let numerator = f32::from(u16::try_from(elapsed.min(u64::from(duration))).unwrap_or(u16::MAX));
    (numerator / denominator).clamp(0.0, 1.0)
}

/// Evaluates a cubic-bezier timing curve by solving its x coordinate with Newton and bisection.
#[must_use]
pub fn cubic_bezier(x: f32, curve: (f32, f32, f32, f32)) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let (x1, y1, x2, y2) = curve;
    let component = |t: f32, p1: f32, p2: f32| {
        3.0 * (1.0 - t).powi(2) * t * p1 + 3.0 * (1.0 - t) * t.powi(2) * p2 + t.powi(3)
    };
    let derivative = |t: f32, p1: f32, p2: f32| {
        3.0 * (1.0 - t).powi(2) * p1
            + 6.0 * (1.0 - t) * t * (p2 - p1)
            + 3.0 * t.powi(2) * (1.0 - p2)
    };
    let mut t = x;
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..6 {
        let error = component(t, x1, x2) - x;
        if error.abs() < 1.0e-6 {
            break;
        }
        if error > 0.0 {
            high = t;
        } else {
            low = t;
        }
        let slope = derivative(t, x1, x2);
        let candidate = if slope.abs() > 1.0e-6 {
            t - error / slope
        } else {
            f32::NAN
        };
        t = if candidate.is_finite() && candidate > low && candidate < high {
            candidate
        } else {
            (low + high) * 0.5
        };
    }
    for _ in 0..12 {
        let error = component(t, x1, x2) - x;
        if error.abs() < 1.0e-6 {
            break;
        }
        if error > 0.0 {
            high = t;
        } else {
            low = t;
        }
        t = (low + high) * 0.5;
    }
    component(t, y1, y2).clamp(0.0, 1.0)
}
