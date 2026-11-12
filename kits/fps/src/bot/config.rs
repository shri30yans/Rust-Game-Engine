//! Bot tuning: the plain numbers a game passes in, and the validated configuration built from them.
//!
//! Invariants: [`BotConfig`] is built only from [`BotParams`] through validation, including when it
//! is decoded; the engage range is never longer than the sight range.

use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::error::KitError;
use crate::validate::{at_least_one, fraction, non_negative, positive};

/// The plain numbers a game passes in for a bot. [`BotConfig::new`] validates them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BotParams {
    /// Farthest enemy a bot can see, in metres.
    pub sight_range: f32,
    /// Full horizontal field of view, in radians, in `(0, 2pi]`.
    pub fov_rad: f32,
    /// Ticks an enemy must stay in sight before the bot may fire.
    pub reaction_ticks: u32,
    /// Rate at which the aim error shrinks while an enemy is seen, in radians per second. The view
    /// itself tracks an enemy at `turn_speed`, so a slow aim error does not stop the bot turning.
    pub aim_speed_rad_per_s: f32,
    /// Starting aim error, in radians. It shrinks at `aim_speed_rad_per_s` while the enemy is seen.
    pub aim_error_rad: f32,
    /// Distance at which a bot in sight switches to `Attack`, in metres, not more than sight range.
    pub engage_range: f32,
    /// Distance at which the bot stops advancing towards a visible enemy, in metres.
    pub stop_distance: f32,
    /// Mean ticks between strafe direction changes while attacking, at least one. Each change
    /// draws the next gap from half to one and a half times this, so a bot is hard to predict.
    pub strafe_period_ticks: u32,
    /// Chance per tick, in `[0, 1]`, that an attacking bot standing on the ground jumps.
    pub jump_chance: f32,
    /// Speed at which the view turns, in radians per second: towards a visible enemy, and towards
    /// the heading when not tracking one.
    pub turn_speed: f32,
    /// Ticks without sight before a bot gives up its enemy and patrols again.
    pub forget_ticks: u32,
    /// Horizontal distance at which a patrolling or routed bot has reached a waypoint, in metres,
    /// greater than zero.
    pub arrive_radius: f32,
    /// Ticks a moving bot may make too little progress before it counts as stuck, at least one.
    pub stuck_ticks: u32,
    /// Horizontal distance a moving bot must cover within `stuck_ticks` to count as progressing,
    /// in metres, zero or more.
    pub stuck_progress: f32,
}

/// Validated bot configuration. Built only from [`BotParams`], including when decoded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BotParams")]
pub struct BotConfig {
    pub(super) sight_range: f32,
    pub(super) fov_rad: f32,
    pub(super) reaction_ticks: u32,
    pub(super) aim_speed_rad_per_s: f32,
    pub(super) aim_error_rad: f32,
    pub(super) engage_range: f32,
    pub(super) stop_distance: f32,
    pub(super) strafe_period_ticks: u32,
    pub(super) jump_chance: f32,
    pub(super) turn_speed: f32,
    pub(super) forget_ticks: u32,
    pub(super) arrive_radius: f32,
    pub(super) stuck_ticks: u32,
    pub(super) stuck_progress: f32,
}

impl TryFrom<BotParams> for BotConfig {
    type Error = KitError;

    fn try_from(params: BotParams) -> Result<Self, Self::Error> {
        let sight_range = positive(params.sight_range, "bot sight range")?;
        let fov_rad = positive(params.fov_rad, "bot field of view")?;
        if fov_rad > TAU {
            return Err(KitError::OutOfRange {
                field: "bot field of view",
                requirement: "at most 2pi",
                value: fov_rad,
            });
        }
        let engage_range = positive(params.engage_range, "bot engage range")?;
        if engage_range > sight_range {
            return Err(KitError::OutOfRange {
                field: "bot engage range",
                requirement: "no more than the sight range",
                value: engage_range,
            });
        }
        Ok(Self {
            sight_range,
            fov_rad,
            reaction_ticks: params.reaction_ticks,
            aim_speed_rad_per_s: positive(params.aim_speed_rad_per_s, "bot aim speed")?,
            aim_error_rad: non_negative(params.aim_error_rad, "bot aim error")?,
            engage_range,
            stop_distance: non_negative(params.stop_distance, "bot stop distance")?,
            strafe_period_ticks: at_least_one(params.strafe_period_ticks, "strafe period")?,
            jump_chance: fraction(params.jump_chance, "bot jump chance")?,
            turn_speed: positive(params.turn_speed, "bot turn speed")?,
            forget_ticks: params.forget_ticks,
            arrive_radius: positive(params.arrive_radius, "bot arrive radius")?,
            stuck_ticks: at_least_one(params.stuck_ticks, "bot stuck ticks")?,
            stuck_progress: non_negative(params.stuck_progress, "bot stuck progress")?,
        })
    }
}

impl BotConfig {
    /// Validates the parameters into a configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] naming the first field that is out of range, not finite, or that
    /// makes the engage range longer than the sight range.
    pub fn new(params: BotParams) -> Result<Self, KitError> {
        Self::try_from(params)
    }
}
