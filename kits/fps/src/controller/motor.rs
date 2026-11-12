//! The physics character shared by every locomotion entity in a world: the [`Motor`] holds the
//! validated tuning and two Rapier character controllers, one standing and one crouched.
//!
//! Invariants: the capsule is taller than twice its radius in both postures; a retune builds both
//! controllers before it replaces the tuning, so a rejected retune changes nothing; the state hash
//! covers every tuning field.

use engine_core::StateHasher;
use engine_physics::{CharacterConfig, CharacterController, ConvexShape};
use engine_world::Resource;

use super::config::ControllerConfig;
use crate::error::KitError;

/// Gap the capsule keeps from geometry, in metres. A physical tolerance, not a gameplay number.
pub(super) const SKIN_WIDTH_M: f32 = 0.01;
/// Distance a descending character snaps to ground, in metres. A physical tolerance.
pub(super) const GROUND_SNAP_M: f32 = 0.15;

/// The physics character shared by every locomotion entity in a world: the movement tuning and
/// the Rapier character controller built from it. Its tuning is part of the state hash.
pub struct Motor {
    pub(super) config: ControllerConfig,
    standing_controller: CharacterController,
    crouched_controller: CharacterController,
    pub(super) capsule_radius: f32,
}

impl Motor {
    /// Builds the character controller for a vertical capsule and a tuning.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError::Physics`] when the capsule is rejected.
    pub fn new(config: ControllerConfig, capsule_radius: f32) -> Result<Self, KitError> {
        if (config.stand_height * 0.5) <= capsule_radius
            || (config.crouch_height * 0.5) <= capsule_radius
        {
            return Err(crate::error::KitError::OutOfRange {
                field: "capsule height",
                requirement: "greater than twice its radius",
                value: capsule_radius * 2.0,
            });
        }
        let standing_half = config.stand_height * 0.5 - capsule_radius;
        let crouched_half = config.crouch_height * 0.5 - capsule_radius;
        let standing_controller = Self::build_controller(&config, standing_half, capsule_radius)?;
        let crouched_controller = Self::build_controller(&config, crouched_half, capsule_radius)?;
        Ok(Self {
            config,
            standing_controller,
            crouched_controller,
            capsule_radius,
        })
    }

    /// The movement tuning.
    #[must_use]
    pub const fn config(&self) -> &ControllerConfig {
        &self.config
    }

    /// Replaces the validated movement tuning without changing locomotion state.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError::Physics`] if Rapier rejects rebuilding the controller. The capsule
    /// dimensions are the same values accepted by [`Motor::new`].
    pub fn set_config(&mut self, config: ControllerConfig) -> Result<(), KitError> {
        let standing_half = config.stand_height * 0.5 - self.capsule_radius;
        let crouched_half = config.crouch_height * 0.5 - self.capsule_radius;
        let standing_controller =
            Self::build_controller(&config, standing_half, self.capsule_radius)?;
        let crouched_controller =
            Self::build_controller(&config, crouched_half, self.capsule_radius)?;
        self.config = config;
        self.standing_controller = standing_controller;
        self.crouched_controller = crouched_controller;
        Ok(())
    }

    pub(super) fn controller(&self, crouched: bool) -> &CharacterController {
        if crouched {
            &self.crouched_controller
        } else {
            &self.standing_controller
        }
    }

    pub(super) fn collider_shape(&self, crouched: bool) -> Result<ConvexShape, KitError> {
        let height = if crouched {
            self.config.crouch_height
        } else {
            self.config.stand_height
        };
        Ok(ConvexShape::capsule(
            height * 0.5 - self.capsule_radius,
            self.capsule_radius,
        )?)
    }

    fn build_controller(
        config: &ControllerConfig,
        capsule_half_height: f32,
        capsule_radius: f32,
    ) -> Result<CharacterController, KitError> {
        let character = CharacterConfig::capsule(capsule_half_height, capsule_radius)?
            .skin_width(SKIN_WIDTH_M)?
            .step_up(config.step_height, STEP_MIN_WIDTH_M)?
            .slope_limit(config.slope_limit)?
            .ground_snap_distance(GROUND_SNAP_M)?
            .vertical_motion(config.jump_speed, config.gravity, config.max_fall_speed)?
            .query_filter(config.query_filter);
        Ok(CharacterController::new(character))
    }
}

/// Narrowest obstacle the character climbs without a jump, in metres. A physical tolerance.
const STEP_MIN_WIDTH_M: f32 = 0.2;

impl Resource for Motor {
    fn hash_into(&self, hasher: &mut StateHasher) {
        let c = &self.config;
        hasher.write_f32(c.walk_speed);
        hasher.write_f32(c.ground_accel);
        hasher.write_f32(c.ground_friction);
        hasher.write_f32(c.air_accel);
        hasher.write_f32(c.air_speed_cap);
        hasher.write_f32(c.jump_speed);
        hasher.write_f32(c.gravity);
        hasher.write_f32(c.max_fall_speed);
        hasher.write_u32(c.coyote_ticks);
        hasher.write_u32(c.jump_buffer_ticks);
        hasher.write_f32(c.jump_cut);
        hasher.write_f32(c.step_height);
        hasher.write_f32(c.slope_limit);
        hasher.write_f32(c.sprint_speed);
        hasher.write_f32(c.crouch_speed);
        hasher.write_f32(c.stand_height);
        hasher.write_f32(c.crouch_height);
        hasher.write_f32(c.slide_min_speed);
        hasher.write_f32(c.slide_boost);
        hasher.write_f32(c.slide_friction);
        hasher.write_u32(u32::from(c.slide_ticks));
        hasher.write_f32(c.mantle_reach);
        hasher.write_f32(c.mantle_min_height);
        hasher.write_f32(c.mantle_max_height);
        hasher.write_u32(u32::from(c.mantle_ticks));
        hasher.write_u32(u32::from(c.camera_crouch_ticks));
    }
}
