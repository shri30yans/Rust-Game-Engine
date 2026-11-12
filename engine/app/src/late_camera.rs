//! Late camera: the render thread applies the newest look input just before it
//! records a frame, and the input sent to the server carries the same angles.
//! Pipelining therefore adds no aim delay.
//!
//! [`ViewAngles`] always holds finite values: yaw is wrapped into `(-PI, PI]` and
//! pitch is clamped to `±PITCH_LIMIT`. [`LookInbox`] is the one shared piece: the
//! input thread adds look deltas, the simulation drains them each tick, and the
//! render thread peeks at what has not been drained yet. Its lock is a plain
//! `Mutex`, and a poisoned lock is an error.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::{Arc, Mutex, MutexGuard};

use glam::Vec2;

use crate::error::AppError;

/// Largest pitch magnitude a view may reach, in radians. Just short of straight up or down.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.001;

/// A view direction in radians. Yaw is wrapped into `(-PI, PI]`, pitch is within `±PITCH_LIMIT`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewAngles {
    yaw: f32,
    pitch: f32,
}

impl ViewAngles {
    /// Yaw 0 and pitch 0: looking down -Z, level.
    pub const ZERO: Self = Self {
        yaw: 0.0,
        pitch: 0.0,
    };

    /// Creates a view from yaw and pitch in radians. Yaw wraps and pitch clamps.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NonFiniteAngle`] when either value is NaN or infinite.
    pub fn new(yaw: f32, pitch: f32) -> Result<Self, AppError> {
        Ok(Self {
            yaw: wrap_yaw(check_finite(yaw)?),
            pitch: clamp_pitch(check_finite(pitch)?),
        })
    }

    /// Yaw in radians, in `(-PI, PI]`.
    #[must_use]
    pub const fn yaw(self) -> f32 {
        self.yaw
    }

    /// Pitch in radians, within `±PITCH_LIMIT`.
    #[must_use]
    pub const fn pitch(self) -> f32 {
        self.pitch
    }

    /// Adds a look delta: yaw wraps, pitch clamps. Pure.
    ///
    /// Yaw is at most `PI` in magnitude before the add, and a finite delta plus that
    /// cannot overflow `f32`, so the sums below are always finite.
    #[must_use]
    pub fn turned(self, delta: LookDelta) -> Self {
        Self {
            yaw: wrap_yaw(self.yaw + delta.yaw),
            pitch: clamp_pitch(self.pitch + delta.pitch),
        }
    }
}

/// A change of view in radians. Both components are finite.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LookDelta {
    yaw: f32,
    pitch: f32,
}

impl LookDelta {
    /// No change.
    pub const ZERO: Self = Self {
        yaw: 0.0,
        pitch: 0.0,
    };

    /// Creates a delta from yaw and pitch change in radians.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NonFiniteAngle`] when either value is NaN or infinite.
    pub fn new(yaw: f32, pitch: f32) -> Result<Self, AppError> {
        Ok(Self {
            yaw: check_finite(yaw)?,
            pitch: check_finite(pitch)?,
        })
    }

    /// Yaw change in radians.
    #[must_use]
    pub const fn yaw(self) -> f32 {
        self.yaw
    }

    /// Pitch change in radians.
    #[must_use]
    pub const fn pitch(self) -> f32 {
        self.pitch
    }
}

/// Radians of view change per device pixel on each axis, used by default (raised
/// from 0.0025, which felt slow on a phone).
const DEFAULT_RADIANS_PER_PIXEL: f32 = 0.004;

/// How far the view turns per pixel of mouse or touch motion, on each axis. Both finite and
/// above zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookSensitivity {
    x: f32,
    y: f32,
}

impl LookSensitivity {
    /// The same sensitivity on both axes, in radians per pixel.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidLookSensitivity`] when the value is NaN, infinite or not above zero.
    pub fn new(radians_per_pixel: f32) -> Result<Self, AppError> {
        Self::per_axis(radians_per_pixel, radians_per_pixel)
    }

    /// A sensitivity for each axis, in radians per pixel: `x` turns left and right, `y` looks up
    /// and down.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidLookSensitivity`] when either value is NaN, infinite or not
    /// above zero.
    pub fn per_axis(x: f32, y: f32) -> Result<Self, AppError> {
        for value in [x, y] {
            if !(value.is_finite() && value > 0.0) {
                return Err(AppError::InvalidLookSensitivity(value));
            }
        }
        Ok(Self { x, y })
    }

    /// Radians of turn per pixel of horizontal motion.
    #[must_use]
    pub const fn x(self) -> f32 {
        self.x
    }

    /// Radians of look per pixel of vertical motion.
    #[must_use]
    pub const fn y(self) -> f32 {
        self.y
    }

    /// Turns motion in device pixels into a view change. Moving right turns the view right and
    /// moving up looks up (screen y grows downward).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NonFiniteAngle`] when the scaled motion overflows.
    pub fn delta(self, mouse_pixels: Vec2) -> Result<LookDelta, AppError> {
        // Yaw grows to the left (see `render::forward`), so a rightward motion subtracts.
        LookDelta::new(-mouse_pixels.x * self.x, -mouse_pixels.y * self.y)
    }
}

impl Default for LookSensitivity {
    fn default() -> Self {
        Self {
            x: DEFAULT_RADIANS_PER_PIXEL,
            y: DEFAULT_RADIANS_PER_PIXEL,
        }
    }
}

/// Accumulated look input shared between the input, simulation and render threads.
#[derive(Debug, Clone, Default)]
pub struct LookInbox {
    inner: Arc<Mutex<LookDelta>>,
}

impl LookInbox {
    /// Creates an empty inbox.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LookDelta::ZERO)),
        }
    }

    /// Adds a delta to the pending total.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NonFiniteAngle`] when the sum would overflow to infinity; the total
    /// is left unchanged then. Returns [`AppError::ExchangePoisoned`] for a poisoned lock.
    pub fn add(&self, delta: LookDelta) -> Result<(), AppError> {
        let mut total = self.lock()?;
        *total = LookDelta::new(total.yaw + delta.yaw, total.pitch + delta.pitch)?;
        Ok(())
    }

    /// Returns the pending total and resets it to zero.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangePoisoned`] for a poisoned lock.
    pub fn drain(&self) -> Result<LookDelta, AppError> {
        let mut total = self.lock()?;
        let taken = *total;
        *total = LookDelta::ZERO;
        Ok(taken)
    }

    /// Returns the pending total without resetting it.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangePoisoned`] for a poisoned lock.
    pub fn peek(&self) -> Result<LookDelta, AppError> {
        Ok(*self.lock()?)
    }

    /// Locks the total, mapping a poisoned lock to an error.
    fn lock(&self) -> Result<MutexGuard<'_, LookDelta>, AppError> {
        self.inner.lock().map_err(|_| AppError::ExchangePoisoned)
    }
}

/// Returns `value` if it is finite, otherwise [`AppError::NonFiniteAngle`].
fn check_finite(value: f32) -> Result<f32, AppError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(AppError::NonFiniteAngle)
    }
}

/// Wraps a finite angle into `(-PI, PI]`.
fn wrap_yaw(yaw: f32) -> f32 {
    let wrapped = yaw.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// Clamps a finite pitch to `±PITCH_LIMIT`.
fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn sensitivity_rejects_zero_negative_and_non_finite_values() {
        for bad in [0.0, -0.001, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    LookSensitivity::new(bad),
                    Err(AppError::InvalidLookSensitivity(_))
                ),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn default_sensitivity_is_the_documented_value() {
        let sensitivity = LookSensitivity::default();
        assert!((sensitivity.x() - 0.004).abs() < 1e-9);
        assert!((sensitivity.y() - 0.004).abs() < 1e-9);
    }

    #[test]
    fn the_axes_scale_independently() {
        let sensitivity = LookSensitivity::per_axis(0.01, 0.02).expect("valid");
        let delta = sensitivity.delta(Vec2::new(10.0, 10.0)).expect("finite");
        assert!((delta.yaw() + 0.1).abs() < 1e-6);
        assert!((delta.pitch() + 0.2).abs() < 1e-6);
        assert!(LookSensitivity::per_axis(0.01, 0.0).is_err());
    }

    #[test]
    fn mouse_right_turns_right_and_mouse_up_looks_up() {
        let sensitivity = LookSensitivity::new(0.01).expect("valid");
        let delta = sensitivity
            .delta(Vec2::new(10.0, -20.0))
            .expect("finite motion");
        assert!((delta.yaw() + 0.1).abs() < 1e-6);
        assert!((delta.pitch() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn zero_motion_gives_no_change() {
        let delta = LookSensitivity::default()
            .delta(Vec2::ZERO)
            .expect("finite motion");
        assert_eq!(delta, LookDelta::ZERO);
    }

    #[test]
    fn overflowing_motion_is_an_error() {
        let sensitivity = LookSensitivity::new(f32::MAX).expect("finite and positive");
        assert!(matches!(
            sensitivity.delta(Vec2::new(10.0, 0.0)),
            Err(AppError::NonFiniteAngle)
        ));
    }
}
