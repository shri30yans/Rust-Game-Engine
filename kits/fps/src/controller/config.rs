//! Movement tuning: the plain numbers a game passes in, and the validated configuration built from
//! them.
//!
//! Invariants: [`ControllerConfig`] is built only by [`ControllerConfig::new`], which checks every
//! field before it is stored: finite values, positive where a positive value is required, a jump
//! cut in `[0, 1]`, a slope strictly between zero and pi/2, crouch below standing, mantle minimum
//! below maximum and within the design ceiling, and sprint above walk above crouch.

use std::f32::consts::FRAC_PI_2;

use engine_physics::QueryFilter;

use crate::error::KitError;
use crate::validate::{finite, fraction, non_negative, positive};

/// Design ceiling for mantle height in metres.
const MAX_MANTLE_HEIGHT_M: f32 = 1.6;
/// The movement tuning a game passes in. Plain data; [`ControllerConfig::new`] validates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerTuning {
    /// Ground speed at full wish, in metres per second.
    pub walk_speed: f32,
    /// Ground acceleration towards the wished velocity, in metres per second squared.
    pub ground_accel: f32,
    /// Deceleration to a stop when there is no wish on the ground, in metres per second squared.
    pub ground_friction: f32,
    /// Air control acceleration, in metres per second squared.
    pub air_accel: f32,
    /// Maximum horizontal speed in the air, in metres per second.
    pub air_speed_cap: f32,
    /// Launch speed of a jump, in metres per second.
    pub jump_speed: f32,
    /// Downward acceleration, in metres per second squared.
    pub gravity: f32,
    /// Terminal fall speed, in metres per second.
    pub max_fall_speed: f32,
    /// Ticks after leaving the ground during which a jump is still allowed.
    pub coyote_ticks: u32,
    /// Ticks a jump press is remembered for, so a press just before landing jumps on landing.
    pub jump_buffer_ticks: u32,
    /// Multiplier on the upward speed when jump is released while rising, in `[0, 1]`.
    pub jump_cut: f32,
    /// Tallest obstacle climbed without a jump, in metres.
    pub step_height: f32,
    /// Steepest walkable slope, in radians, strictly between zero and pi/2.
    pub slope_limit: f32,
    /// Ground sprint speed in metres per second.
    pub sprint_speed: f32,
    /// Ground crouch speed in metres per second.
    pub crouch_speed: f32,
    /// Total standing capsule height in metres.
    pub stand_height: f32,
    /// Total crouched capsule height in metres.
    pub crouch_height: f32,
    /// Minimum horizontal speed to begin a slide.
    pub slide_min_speed: f32,
    /// One-time slide speed boost in metres per second.
    pub slide_boost: f32,
    /// Horizontal deceleration while sliding.
    pub slide_friction: f32,
    /// Slide duration in ticks.
    pub slide_ticks: u16,
    /// Forward mantle reach in metres.
    pub mantle_reach: f32,
    /// Minimum ledge height in metres.
    pub mantle_min_height: f32,
    /// Maximum ledge height in metres.
    pub mantle_max_height: f32,
    /// Mantle duration in ticks.
    pub mantle_ticks: u16,
    /// Camera eye-height blend duration in ticks.
    pub camera_crouch_ticks: u16,
}

/// Validated movement tuning. Built only by [`ControllerConfig::new`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerConfig {
    pub(super) walk_speed: f32,
    pub(super) ground_accel: f32,
    pub(super) ground_friction: f32,
    pub(super) air_accel: f32,
    pub(super) air_speed_cap: f32,
    pub(super) jump_speed: f32,
    pub(super) gravity: f32,
    pub(super) max_fall_speed: f32,
    pub(super) coyote_ticks: u32,
    pub(super) jump_buffer_ticks: u32,
    pub(super) jump_cut: f32,
    pub(super) step_height: f32,
    pub(super) slope_limit: f32,
    pub(super) sprint_speed: f32,
    pub(super) crouch_speed: f32,
    pub(super) stand_height: f32,
    pub(super) crouch_height: f32,
    pub(super) slide_min_speed: f32,
    pub(super) slide_boost: f32,
    pub(super) slide_friction: f32,
    pub(super) slide_ticks: u16,
    pub(super) mantle_reach: f32,
    pub(super) mantle_min_height: f32,
    pub(super) mantle_max_height: f32,
    pub(super) mantle_ticks: u16,
    pub(super) camera_crouch_ticks: u16,
    pub(super) query_filter: QueryFilter,
}

impl ControllerConfig {
    /// Validates a tuning and returns the configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] naming the first field that is not finite, not positive where
    /// positive is required, outside `[0, 1]` for `jump_cut`, or outside `(0, pi/2)` for the slope.
    pub fn new(tuning: ControllerTuning) -> Result<Self, KitError> {
        let slope_limit = finite(tuning.slope_limit, "slope limit")?;
        if slope_limit <= 0.0 || slope_limit >= FRAC_PI_2 {
            return Err(KitError::OutOfRange {
                field: "slope limit",
                requirement: "greater than zero and less than pi/2",
                value: slope_limit,
            });
        }
        let stand_height = positive(tuning.stand_height, "standing height")?;
        let crouch_height = positive(tuning.crouch_height, "crouch height")?;
        if crouch_height >= stand_height {
            return Err(crate::error::KitError::OutOfRange {
                field: "crouch height",
                requirement: "less than standing height",
                value: crouch_height,
            });
        }
        if tuning.mantle_min_height >= tuning.mantle_max_height {
            return Err(crate::error::KitError::OutOfRange {
                field: "mantle minimum height",
                requirement: "less than mantle maximum height",
                value: tuning.mantle_min_height,
            });
        }
        if tuning.mantle_max_height > MAX_MANTLE_HEIGHT_M {
            return Err(crate::error::KitError::OutOfRange {
                field: "mantle maximum height",
                requirement: "at most 1.6 metres",
                value: tuning.mantle_max_height,
            });
        }
        let walk_speed = positive(tuning.walk_speed, "walk speed")?;
        let sprint_speed = positive(tuning.sprint_speed, "sprint speed")?;
        let crouch_speed = positive(tuning.crouch_speed, "crouch speed")?;
        if sprint_speed <= walk_speed {
            return Err(crate::error::KitError::OutOfRange {
                field: "sprint speed",
                requirement: "greater than walking speed",
                value: sprint_speed,
            });
        }
        if crouch_speed >= walk_speed {
            return Err(crate::error::KitError::OutOfRange {
                field: "crouch speed",
                requirement: "less than walking speed",
                value: crouch_speed,
            });
        }
        for (field, value) in [
            ("slide duration", tuning.slide_ticks),
            ("mantle duration", tuning.mantle_ticks),
            ("camera crouch duration", tuning.camera_crouch_ticks),
        ] {
            if value == 0 {
                return Err(KitError::CountOutOfRange {
                    field,
                    requirement: "at least one tick",
                    value: 0,
                });
            }
        }
        Ok(Self {
            walk_speed,
            ground_accel: positive(tuning.ground_accel, "ground acceleration")?,
            ground_friction: positive(tuning.ground_friction, "ground friction")?,
            air_accel: positive(tuning.air_accel, "air acceleration")?,
            air_speed_cap: positive(tuning.air_speed_cap, "air speed cap")?,
            jump_speed: positive(tuning.jump_speed, "jump speed")?,
            gravity: positive(tuning.gravity, "gravity")?,
            max_fall_speed: positive(tuning.max_fall_speed, "max fall speed")?,
            coyote_ticks: tuning.coyote_ticks,
            jump_buffer_ticks: tuning.jump_buffer_ticks,
            jump_cut: fraction(tuning.jump_cut, "jump cut")?,
            step_height: non_negative(tuning.step_height, "step height")?,
            slope_limit,
            sprint_speed,
            crouch_speed,
            stand_height,
            crouch_height,
            slide_min_speed: non_negative(tuning.slide_min_speed, "slide minimum speed")?,
            slide_boost: non_negative(tuning.slide_boost, "slide boost")?,
            slide_friction: positive(tuning.slide_friction, "slide friction")?,
            slide_ticks: tuning.slide_ticks,
            mantle_reach: positive(tuning.mantle_reach, "mantle reach")?,
            mantle_min_height: positive(tuning.mantle_min_height, "mantle minimum height")?,
            mantle_max_height: positive(tuning.mantle_max_height, "mantle maximum height")?,
            mantle_ticks: tuning.mantle_ticks,
            camera_crouch_ticks: tuning.camera_crouch_ticks,
            query_filter: QueryFilter::default(),
        })
    }

    /// Selects the collider layers used for character movement and mantle queries.
    #[must_use]
    pub const fn query_filter(mut self, filter: QueryFilter) -> Self {
        self.query_filter = filter;
        self
    }

    /// Ground speed at full wish, in metres per second.
    #[must_use]
    pub const fn walk_speed(&self) -> f32 {
        self.walk_speed
    }

    /// Maximum horizontal speed in the air, in metres per second.
    #[must_use]
    pub const fn air_speed_cap(&self) -> f32 {
        self.air_speed_cap
    }

    /// Launch speed of a jump, in metres per second.
    #[must_use]
    pub const fn jump_speed(&self) -> f32 {
        self.jump_speed
    }

    /// Terminal fall speed, in metres per second.
    #[must_use]
    pub const fn max_fall_speed(&self) -> f32 {
        self.max_fall_speed
    }

    /// Speed while sprinting, in metres per second.
    #[must_use]
    pub const fn sprint_speed(&self) -> f32 {
        self.sprint_speed
    }

    /// Speed while crouched, in metres per second.
    #[must_use]
    pub const fn crouch_speed(&self) -> f32 {
        self.crouch_speed
    }

    /// Total standing capsule height, in metres.
    #[must_use]
    pub const fn stand_height(&self) -> f32 {
        self.stand_height
    }

    /// Total crouched capsule height, in metres.
    #[must_use]
    pub const fn crouch_height(&self) -> f32 {
        self.crouch_height
    }
}
