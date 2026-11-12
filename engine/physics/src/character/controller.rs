//! The shared kinematic character controller and its pure fixed-step move.
//!
//! `move_character` is a pure function of its explicit state, input, world and fixed configuration.
//! It never mutates the world, and a move that would end embedded in geometry keeps the character's
//! valid start pose.

use super::config::CharacterConfig;
use super::ground::{ground_snap, steep_ground_below};
use super::input::CharacterInput;
use super::state::{CharacterMovement, CharacterState};
use crate::convert::{from_rapier_vec, to_rapier_pose, to_rapier_vec};
use crate::{BodyId, PhysicsWorld, QueryFilter};
use glam::{Quat, Vec3};
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::{Collider, ColliderHandle, SharedShape};

/// Immutable shared kinematic character controller.
pub struct CharacterController {
    config: CharacterConfig,
    shape: SharedShape,
    /// The capsule inset by the skin width: overlapping anything with it means the character is
    /// embedded deeper than the separation the controller keeps.
    clearance: SharedShape,
    rapier_controller: KinematicCharacterController,
}

impl CharacterController {
    /// Creates a controller and caches its immutable Rapier shape and movement configuration.
    #[must_use]
    pub fn new(config: CharacterConfig) -> Self {
        let shape = config.shape().shared();
        let clearance = SharedShape::capsule_y(
            config.capsule_half_height,
            config.capsule_radius - config.skin_width,
        );
        let rapier_controller = KinematicCharacterController {
            up: to_rapier_vec(Vec3::Y),
            offset: CharacterLength::Absolute(config.skin_width),
            slide: true,
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(config.step_height),
                min_width: CharacterLength::Absolute(config.step_min_width),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: config.slope_limit_radians,
            min_slope_slide_angle: config.slope_limit_radians,
            snap_to_ground: Some(CharacterLength::Absolute(config.ground_snap_distance)),
            normal_nudge_factor: config.skin_width,
        };
        Self {
            config,
            shape,
            clearance,
            rapier_controller,
        }
    }

    /// Returns the immutable controller configuration.
    #[must_use]
    pub const fn config(&self) -> CharacterConfig {
        self.config
    }

    /// Whether the surface under the capsule centred at `position` is steeper than the slope limit.
    /// `false` when no surface is in reach (the character is in the air).
    fn ends_on_steep_surface(
        &self,
        world: &PhysicsWorld,
        filter: QueryFilter,
        position: Vec3,
    ) -> bool {
        steep_ground_below(world, filter, &self.config, position)
    }

    /// Whether the capsule centred at `position` overlaps geometry deeper than the skin width.
    fn embedded(&self, world: &PhysicsWorld, filter: QueryFilter, position: Vec3) -> bool {
        let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
        let query_filter = filter.rapier_filter(&predicate);
        world
            .query_pipeline(query_filter)
            .intersect_shape(
                to_rapier_pose(position, Quat::IDENTITY),
                self.clearance.as_ref(),
            )
            .next()
            .is_some()
    }

    /// Computes one movement from explicit state, input, world, and the moving character body.
    ///
    /// This does not mutate the physics world or hide character state internally.
    /// Every movement, steep-surface, and ground-snap query excludes `body` and its colliders.
    /// Callers may apply the returned pose to a kinematic body with
    /// [`PhysicsWorld::set_next_kinematic_pose`].
    #[must_use]
    pub fn move_character(
        &self,
        state: CharacterState,
        input: CharacterInput,
        world: &PhysicsWorld,
        body: BodyId,
    ) -> CharacterMovement {
        let mut filter = self.config.query_filter;
        filter.exclude_body = Some(body);
        self.move_filtered(state, input, world, filter)
    }

    /// The movement computed with every query made under `filter`.
    fn move_filtered(
        &self,
        state: CharacterState,
        input: CharacterInput,
        world: &PhysicsWorld,
        filter: QueryFilter,
    ) -> CharacterMovement {
        let dt = world.config().dt();
        let mut vertical_velocity = if input.jump && state.grounded {
            self.config.jump_speed
        } else if state.grounded && state.vertical_velocity < 0.0 {
            0.0
        } else {
            state.vertical_velocity
        };
        vertical_velocity =
            (vertical_velocity - self.config.gravity * dt).max(-self.config.max_fall_speed);

        let vertical_part = Vec3::Y * (vertical_velocity * dt);
        let desired_translation = input.ground_velocity * dt + vertical_part;
        let character_pose = to_rapier_pose(state.position, Quat::IDENTITY);
        let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
        let query_filter = filter.rapier_filter(&predicate);
        let queries = world.query_pipeline(query_filter);
        let mut movement = self.rapier_controller.move_shape(
            dt,
            &queries,
            self.shape.as_ref(),
            &character_pose,
            to_rapier_vec(desired_translation),
            |_| {},
        );
        let mut translation = from_rapier_vec(movement.translation);
        if translation.y > 0.0
            && self.ends_on_steep_surface(world, filter, state.position + translation)
        {
            // Rapier slides a blocked move along a non-walkable face, and that slide can carry
            // the character up the face (measured: a 50 degree ramp at limit 45 raised the feet to
            // 0.57 m, with autostep both on and off). Horizontal input into such a face is
            // rejected; the vertical part alone (gravity, or the jump) is applied.
            movement = self.rapier_controller.move_shape(
                dt,
                &queries,
                self.shape.as_ref(),
                &character_pose,
                to_rapier_vec(vertical_part),
                |_| {},
            );
            translation = from_rapier_vec(movement.translation);
        }
        let mut grounded = movement.grounded;
        // A snap only pulls down a character that is falling or resting, and not already grounded.
        let may_snap = !grounded && vertical_velocity <= 0.0 && translation.y <= 0.0;
        let snap = if may_snap {
            ground_snap(
                world,
                filter,
                &self.config,
                self.shape.as_ref(),
                state.position + translation,
            )
        } else {
            None
        };
        if let Some(snap_down) = snap {
            translation += snap_down;
            grounded = true;
        }
        if self.embedded(world, filter, state.position + translation) {
            // Rapier can resolve a move wedged between two surfaces (for example the floor and an
            // overhanging face) into a pose inside one of them (measured: 3.7 cm into the floor
            // under a box tilted 30 degrees over the character). A move that would end
            // embedded is not taken: the character keeps its valid start pose.
            translation = Vec3::ZERO;
            grounded = state.grounded;
        }
        let end_position = state.position + translation;
        if grounded && vertical_velocity < 0.0 {
            vertical_velocity = 0.0;
        }
        CharacterMovement {
            state: CharacterState {
                position: end_position,
                vertical_velocity,
                grounded,
            },
            translation,
        }
    }
}

impl Clone for CharacterController {
    fn clone(&self) -> Self {
        Self::new(self.config)
    }
}

impl core::fmt::Debug for CharacterController {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CharacterController")
            .field("config", &self.config)
            .finish()
    }
}

impl PartialEq for CharacterController {
    fn eq(&self, other: &Self) -> bool {
        self.config == other.config
    }
}
