//! The pure movement rule: [`step_velocity`] advances a [`LocomotionState`] by one tick from a
//! command and returns that tick's velocity. It touches no physics and no world.
//!
//! Invariants: the horizontal speed never exceeds `max(walk_speed, air_speed_cap)` in the air; the
//! vertical speed stays in `[-max_fall_speed, jump_speed]`; the jump counters never go negative;
//! the jump cut is applied once per jump.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::config::ControllerConfig;
use crate::command::MoveCommand;
use crate::movement::MoveState;

/// The evolving movement state of one character, carried between ticks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct LocomotionState {
    /// Horizontal velocity on the world XZ plane, in metres per second.
    pub(super) horizontal: Vec2,
    /// Vertical speed, in metres per second; positive is up.
    pub(super) vertical: f32,
    /// Ticks of jump allowance left after leaving the ground.
    pub(super) coyote_left: u32,
    /// Ticks a jump press is still remembered for.
    pub(super) buffer_left: u32,
    /// True from a jump until the jump is cut or the character lands.
    pub(super) jumped: bool,
}

impl LocomotionState {
    /// Vertical speed, in metres per second; positive is up.
    #[must_use]
    pub const fn vertical(&self) -> f32 {
        self.vertical
    }

    /// Horizontal velocity on the world XZ plane, in metres per second.
    #[must_use]
    pub const fn horizontal(&self) -> Vec2 {
        self.horizontal
    }
}

/// Advances the movement state by one tick and returns the velocity for that tick.
///
/// The returned vector is the tick's mean velocity: horizontal in `x` and `z`, and vertical in
/// `y` as the average of the speed before and after gravity, so integrating it over the tick
/// gives the displacement. The state keeps the speed after gravity.
///
/// `grounded` is whether the character stood on walkable ground at the end of the last tick.
pub fn step_velocity(
    state: &mut LocomotionState,
    cfg: &ControllerConfig,
    cmd: &MoveCommand,
    grounded: bool,
    dt: f32,
) -> Vec3 {
    step_velocity_in_mode(state, cfg, cmd, grounded, dt, MoveState::Walk)
}

/// Advances velocity using the active ground mode while keeping jump and coyote rules shared.
pub(super) fn step_velocity_in_mode(
    state: &mut LocomotionState,
    cfg: &ControllerConfig,
    cmd: &MoveCommand,
    grounded: bool,
    dt: f32,
    mode: MoveState,
) -> Vec3 {
    if grounded {
        state.coyote_left = cfg.coyote_ticks;
        state.jumped = false;
        if state.vertical < 0.0 {
            state.vertical = 0.0;
        }
    }
    if cmd.jump_pressed() {
        state.buffer_left = cfg.jump_buffer_ticks;
    }
    let can_jump = grounded || state.coyote_left > 0;
    if state.buffer_left > 0 && can_jump {
        state.vertical = cfg.jump_speed;
        state.jumped = true;
        state.buffer_left = 0;
        state.coyote_left = 0;
    } else {
        state.buffer_left = state.buffer_left.saturating_sub(1);
        if !grounded {
            state.coyote_left = state.coyote_left.saturating_sub(1);
        }
    }

    // Variable jump height: releasing the button while rising cuts the speed once.
    if state.jumped && !cmd.jump_held() && state.vertical > 0.0 {
        state.vertical *= cfg.jump_cut;
        state.jumped = false;
    }

    let before = state.vertical;
    let after = (before - cfg.gravity * dt).max(-cfg.max_fall_speed);
    state.vertical = after;
    let mean_vertical = 0.5 * (before + after);

    let wish = cmd.wish();
    let (sin, cos) = cmd.view().yaw().sin_cos();
    // Right is (cos, -sin) and forward is (-sin, -cos) on the XZ plane, as the sandbox defines them.
    let speed = match mode {
        MoveState::Sprint => cfg.sprint_speed,
        MoveState::Crouch | MoveState::Slide { .. } => cfg.crouch_speed,
        _ => cfg.walk_speed,
    };
    let target = Vec2::new(cos * wish.x - sin * wish.y, -sin * wish.x - cos * wish.y) * speed;
    if grounded {
        if matches!(mode, MoveState::Slide { .. }) {
            state.horizontal = move_towards(state.horizontal, Vec2::ZERO, cfg.slide_friction * dt);
        } else {
            let rate = if wish == Vec2::ZERO {
                cfg.ground_friction
            } else {
                cfg.ground_accel
            };
            state.horizontal = move_towards(state.horizontal, target, rate * dt);
        }
    } else {
        state.horizontal = move_towards(state.horizontal, target, cfg.air_accel * dt)
            .clamp_length_max(cfg.air_speed_cap);
    }
    Vec3::new(state.horizontal.x, mean_vertical, state.horizontal.y)
}

/// Moves `current` towards `target` by at most `max_step`, without overshooting.
fn move_towards(current: Vec2, target: Vec2, max_step: f32) -> Vec2 {
    let delta = target - current;
    let distance = delta.length();
    if distance <= max_step {
        target
    } else {
        current + delta * (max_step / distance)
    }
}
