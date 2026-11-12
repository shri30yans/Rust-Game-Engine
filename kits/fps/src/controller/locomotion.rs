//! The per-character component: [`Locomotion`] holds one character's movement state, capsule
//! centre, kinematic body, eye height and the command it is driven by.
//!
//! Invariants: the centre is finite and the eye offset is finite and not negative. The command is
//! input, so it is neither serialised nor hashed.

use engine_physics::{BodyId, ColliderId};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::velocity::LocomotionState;
use crate::command::MoveCommand;
use crate::error::KitError;
use crate::movement::MoveState;
use crate::validate::non_negative;

/// One locomotion actor: its movement state, its capsule centre, its kinematic body, its eye
/// height and the command it is currently driven by.
///
/// The command is input, not state, so it is not serialised or hashed; the simulation reads it
/// each tick, as the sandbox does with its player command.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Locomotion {
    pub(super) state: LocomotionState,
    pub(super) move_state: MoveState,
    pub(super) centre: Vec3,
    pub(super) grounded: bool,
    pub(super) body: BodyId,
    pub(super) collider: ColliderId,
    pub(super) eye_above_centre: f32,
    #[serde(skip)]
    pub(super) command: MoveCommand,
}

impl Locomotion {
    /// A character at rest at `centre`, driven by `body`, with its eye `eye_above_centre` metres
    /// above the capsule centre.
    ///
    /// # Errors
    ///
    /// Returns an error when the centre is not finite or the eye offset is negative or not finite.
    pub fn new(
        body: BodyId,
        collider: ColliderId,
        centre: Vec3,
        eye_above_centre: f32,
    ) -> Result<Self, KitError> {
        if !centre.is_finite() {
            return Err(KitError::NonFiniteVector {
                field: "locomotion centre",
            });
        }
        Ok(Self {
            state: LocomotionState::default(),
            move_state: MoveState::Walk,
            centre,
            grounded: false,
            body,
            collider,
            eye_above_centre: non_negative(eye_above_centre, "eye height above centre")?,
            command: MoveCommand::IDLE,
        })
    }

    /// The movement state after the last tick.
    #[must_use]
    pub const fn state(&self) -> LocomotionState {
        self.state
    }

    /// The mutually exclusive movement mode after the last tick.
    #[must_use]
    pub const fn move_state(&self) -> MoveState {
        self.move_state
    }

    /// The capsule centre after the last tick, in metres.
    #[must_use]
    pub const fn centre(&self) -> Vec3 {
        self.centre
    }

    /// Synchronizes the cached centre after physics preserves the capsule feet during a shape change.
    pub fn sync_centre(&mut self, centre: Vec3) -> Result<(), KitError> {
        if !centre.is_finite() {
            return Err(KitError::NonFiniteVector {
                field: "locomotion centre",
            });
        }
        self.centre = centre;
        Ok(())
    }

    /// Whether the last tick ended on walkable ground.
    #[must_use]
    pub const fn grounded(&self) -> bool {
        self.grounded
    }

    /// The kinematic body that moves this character.
    #[must_use]
    pub const fn body(&self) -> BodyId {
        self.body
    }

    /// The capsule collider whose shape follows the movement mode.
    #[must_use]
    pub const fn collider(&self) -> ColliderId {
        self.collider
    }

    /// Eye height above the capsule centre, in metres.
    #[must_use]
    pub const fn eye_above_centre(&self) -> f32 {
        self.eye_above_centre
    }

    /// The eye position: the centre raised by the eye offset.
    #[must_use]
    pub fn eye(&self) -> Vec3 {
        self.centre + Vec3::Y * self.eye_above_centre
    }

    /// The command this character is driven by.
    #[must_use]
    pub const fn command(&self) -> MoveCommand {
        self.command
    }

    /// Replaces the command for the next tick.
    pub const fn set_command(&mut self, command: MoveCommand) {
        self.command = command;
    }
}
