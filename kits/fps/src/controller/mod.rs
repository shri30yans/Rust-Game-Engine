//! First-person locomotion: the pure movement rules, the component that holds one character, and
//! the system that moves its body through the physics character controller.
//!
//! The pure rule [`step_velocity`] owns the horizontal speed (ground acceleration, friction, air
//! control with a speed cap), the vertical speed (jump, variable jump height, gravity, terminal
//! fall), coyote time and jump buffering. It returns the velocity for the tick. The system then
//! sweeps the capsule with `CharacterController::move_character` and writes the body and the
//! transform back.
//!
//! The physics controller adds one gravity step of its own inside `move_character`. The system
//! cancels that step, so the displacement it applies is exactly the velocity [`step_velocity`]
//! returned, and the vertical speed has a single owner.
//!
//! The world's [`Motor`] can be retuned while running. Its character geometry is retained, and
//! individual [`Locomotion`] velocity, jump and command state stays in place.
//!
//! Invariants: the horizontal speed never exceeds `max(walk_speed, air_speed_cap)`; the vertical
//! speed stays in `[-max_fall_speed, jump_speed]`; the jump counters never go negative; the jump
//! cut is applied once per jump; a dead actor's command is ignored. Every number is validated in
//! [`ControllerConfig::new`].
//!
//! The folder splits the parts by job: `config` (tuning), `velocity` (the pure rule), `motor` (the
//! physics controllers), `locomotion` (the per-character component), `drive` (one character's
//! tick), `mantle` (ledge detection) and `system` (the schedule-facing entry points).

mod config;
mod drive;
mod locomotion;
mod mantle;
mod motor;
mod system;
mod velocity;

#[cfg(test)]
mod tests;

pub use config::{ControllerConfig, ControllerTuning};
pub use locomotion::Locomotion;
pub use motor::Motor;
pub use system::{clear_movement_log_system, locomotion_system, move_one};
pub use velocity::{LocomotionState, step_velocity};
