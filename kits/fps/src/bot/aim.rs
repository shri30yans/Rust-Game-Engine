//! Aim: turning the view towards an enemy or a heading, the aim error a bot carries, and the angle
//! helpers that convert between a direction and the view convention.
//!
//! Invariants: a turn moves the view by at most `turn_speed * dt` on each axis in a tick; wrapped
//! angles stay in `(-PI, PI]`.

use std::f32::consts::{PI, TAU};

use app::{LookDelta, ViewAngles};
use glam::Vec3;

use super::brain::{BotBrain, BotInput};
use super::config::BotConfig;
use crate::error::KitError;

impl BotBrain {
    /// Turns the view this tick and reports whether it reached its desired direction within the
    /// tick. While the enemy is visible the view tracks it at turn speed, offset by the aim error;
    /// otherwise it turns towards `heading` at turn speed, and holds still with no heading.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the turn is not a finite look delta.
    pub(super) fn turn(
        &self,
        cfg: &BotConfig,
        input: &BotInput,
        visible: bool,
        heading: Vec3,
    ) -> Result<(ViewAngles, bool), KitError> {
        let (desired_yaw, desired_pitch, rate) = match input.target {
            Some(target) if visible => {
                let (yaw, pitch) = angles_towards(target.eye - input.eye);
                let yaw = yaw + self.aim_error * self.aim_side.cos();
                let pitch = pitch + self.aim_error * self.aim_side.sin();
                (yaw, pitch, cfg.turn_speed)
            }
            _ if heading.length_squared() > 0.0 => {
                let (yaw, pitch) = angles_towards(heading);
                (yaw, pitch, cfg.turn_speed)
            }
            _ => (input.view.yaw(), input.view.pitch(), cfg.turn_speed),
        };
        let max_step = (rate * input.dt).max(0.0);
        let yaw_error = wrap_angle(desired_yaw - input.view.yaw());
        let pitch_error = desired_pitch - input.view.pitch();
        let aimed = yaw_error.abs() <= max_step && pitch_error.abs() <= max_step;
        let delta = LookDelta::new(
            yaw_error.clamp(-max_step, max_step),
            pitch_error.clamp(-max_step, max_step),
        )?;
        Ok((input.view.turned(delta), aimed))
    }
}

/// The yaw and pitch that look along `direction`, in the view convention of [`ViewAngles`].
/// A zero direction gives yaw and pitch zero.
pub(super) fn angles_towards(direction: Vec3) -> (f32, f32) {
    let yaw = (-direction.x).atan2(-direction.z);
    let pitch = direction.y.atan2(direction.x.hypot(direction.z));
    (yaw, pitch)
}

/// Wraps an angle into `(-PI, PI]`.
pub(super) fn wrap_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}
