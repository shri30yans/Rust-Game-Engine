//! The one command type that drives a player or a bot.
//!
//! A [`MoveCommand`] is built only through [`MoveCommand::new`], so its move axis is clamped to
//! `[-1, 1]` on each axis and its view is the engine's validated [`ViewAngles`]. The game maps its
//! input onto this type, and the bot brain writes this type too, so both use the same systems.
//!
//! Invariants: every field is finite; the wish has length at most one (diagonals are not faster).

use app::ViewAngles;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::error::KitError;
use crate::validate::finite_vec2;

/// Trigger intent resolved by the weapon at tick time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    /// The actor is not pulling the trigger.
    Released,
    /// Fire whenever the weapon is ready.
    Held,
    /// Fire only when the weapon's aim ray first hits a living enemy.
    OnTarget,
}

/// One tick of intent for an actor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawMoveCommand", into = "RawMoveCommand")]
pub struct MoveCommand {
    move_axis: Vec2,
    view: ViewAngles,
    jump_pressed: bool,
    jump_held: bool,
    trigger: Trigger,
    reload: bool,
    sprint: bool,
    crouch: bool,
}

/// Untrusted wire shape. Decoding always passes through [`MoveCommand::new`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RawMoveCommand {
    move_x: f32,
    move_y: f32,
    yaw: f32,
    pitch: f32,
    jump_pressed: bool,
    jump_held: bool,
    trigger: Trigger,
    reload: bool,
    sprint: bool,
    crouch: bool,
}

impl TryFrom<RawMoveCommand> for MoveCommand {
    type Error = KitError;

    fn try_from(raw: RawMoveCommand) -> Result<Self, Self::Error> {
        let view = ViewAngles::new(raw.yaw, raw.pitch)?;
        Ok(Self::new(
            Vec2::new(raw.move_x, raw.move_y),
            view,
            raw.jump_pressed,
            raw.jump_held,
            raw.trigger,
            raw.reload,
        )?
        .with_movement(raw.sprint, raw.crouch))
    }
}

impl From<MoveCommand> for RawMoveCommand {
    fn from(command: MoveCommand) -> Self {
        Self {
            move_x: command.move_axis.x,
            move_y: command.move_axis.y,
            yaw: command.view.yaw(),
            pitch: command.view.pitch(),
            jump_pressed: command.jump_pressed,
            jump_held: command.jump_held,
            trigger: command.trigger,
            reload: command.reload,
            sprint: command.sprint,
            crouch: command.crouch,
        }
    }
}

impl MoveCommand {
    /// No movement, no buttons, facing down -Z.
    pub const IDLE: Self = Self {
        move_axis: Vec2::ZERO,
        view: ViewAngles::ZERO,
        jump_pressed: false,
        jump_held: false,
        trigger: Trigger::Released,
        reload: false,
        sprint: false,
        crouch: false,
    };

    /// Builds a command. `move_axis.x` is strafe (right positive) and `move_axis.y` is forward
    /// (ahead positive); each component is clamped to `[-1, 1]`.
    ///
    /// `jump_pressed` is true only on the tick the button goes down. `jump_held` is the level of
    /// the button; releasing it while rising cuts the jump.
    ///
    /// # Errors
    ///
    /// Returns [`KitError::NonFiniteVector`] when the move axis is not finite.
    pub fn new(
        move_axis: Vec2,
        view: ViewAngles,
        jump_pressed: bool,
        jump_held: bool,
        trigger: Trigger,
        reload: bool,
    ) -> Result<Self, KitError> {
        let move_axis = finite_vec2(move_axis, "move axis")?.clamp(Vec2::splat(-1.0), Vec2::ONE);
        Ok(Self {
            move_axis,
            view,
            jump_pressed,
            jump_held,
            trigger,
            reload,
            sprint: false,
            crouch: false,
        })
    }

    /// Sets the held movement intentions. Crouch is a level; input devices decide whether it is
    /// latched or held, so every simulation tick sees the same command shape.
    #[must_use]
    pub const fn with_movement(self, sprint: bool, crouch: bool) -> Self {
        Self {
            sprint,
            crouch,
            ..self
        }
    }

    /// The move axis, each component in `[-1, 1]`.
    #[must_use]
    pub const fn move_axis(self) -> Vec2 {
        self.move_axis
    }

    /// The view the actor faces and aims along.
    #[must_use]
    pub const fn view(self) -> ViewAngles {
        self.view
    }

    /// Whether the jump button went down this tick.
    #[must_use]
    pub const fn jump_pressed(self) -> bool {
        self.jump_pressed
    }

    /// Whether the jump button is held this tick.
    #[must_use]
    pub const fn jump_held(self) -> bool {
        self.jump_held
    }

    /// Whether the trigger is held this tick.
    #[must_use]
    pub const fn fire(self) -> bool {
        !matches!(self.trigger, Trigger::Released)
    }

    /// Trigger intent, resolved by the weapon system at tick time.
    #[must_use]
    pub const fn trigger(self) -> Trigger {
        self.trigger
    }

    /// Whether reload was requested this tick.
    #[must_use]
    pub const fn reload(self) -> bool {
        self.reload
    }

    /// Whether sprint is wanted this tick.
    #[must_use]
    pub const fn sprint(self) -> bool {
        self.sprint
    }

    /// Whether crouch is wanted this tick.
    #[must_use]
    pub const fn crouch(self) -> bool {
        self.crouch
    }

    /// The horizontal wish: the move axis with length at most one.
    #[must_use]
    pub fn wish(self) -> Vec2 {
        self.move_axis.clamp_length_max(1.0)
    }
}

impl Default for MoveCommand {
    fn default() -> Self {
        Self::IDLE
    }
}

impl engine_netcode::InputPayload for MoveCommand {
    const MAX_BYTES: usize = 64;

    fn validate(&self) -> Result<(), engine_netcode::NetError> {
        Ok(())
    }
}

/// The unit direction a view looks along. Yaw 0 looks down -Z and positive pitch looks up, the
/// same convention as the render camera, so a shot leaves where the player sees.
#[must_use]
pub fn look_direction(view: ViewAngles) -> Vec3 {
    let (yaw_sin, yaw_cos) = view.yaw().sin_cos();
    let (pitch_sin, pitch_cos) = view.pitch().sin_cos();
    Vec3::new(-yaw_sin * pitch_cos, pitch_sin, -yaw_cos * pitch_cos)
}
