//! One character's tick: [`drive`] reads the character, decides its mode, runs the pure velocity
//! rule, sweeps the capsule through the physics controller, and writes the body, the state and the
//! transform back.
//!
//! Invariants: a dead actor keeps its facing and loses every button; a mode change is logged once
//! per transition; the kinematic pose and the transform are written from the same centre.

use engine_physics::{CharacterInput, CharacterState, PhysicsWorld, RayCast};
use engine_world::{NetId, Transform, World, WorldError};
use glam::{Quat, Vec2, Vec3};

use super::locomotion::Locomotion;
use super::mantle::detect_mantle;
use super::motor::{GROUND_SNAP_M, Motor, SKIN_WIDTH_M};
use super::velocity::step_velocity_in_mode;
use crate::command::{MoveCommand, Trigger};
use crate::error::lift;
use crate::health::is_alive;
use crate::movement::{MoveState, MovementEvent, MovementLog, TransitionFacts, next_state};

/// Eye clearance above the capsule's top in metres.
const EYE_CLEARANCE_M: f32 = 0.1;

/// Advances one locomotion entity by one tick.
pub(super) fn drive(world: &mut World, id: NetId, dt: f32) -> Result<(), WorldError> {
    let locomotion = *world.get::<Locomotion>(id)?;
    let alive = is_alive(world, id)?;
    let command = if alive {
        locomotion.command
    } else {
        // A dead actor keeps its facing and loses every button.
        lift(MoveCommand::new(
            Vec2::ZERO,
            locomotion.command.view(),
            false,
            false,
            Trigger::Released,
            false,
        ))?
    };
    let mut state = locomotion.state;
    let previous_mode = locomotion.move_state;
    let (config, collider_shape, standing_collider_shape, crouch_ticks, capsule_radius) = {
        let motor = world.resource::<Motor>()?;
        (
            motor.config,
            lift(motor.collider_shape(true))?,
            lift(motor.collider_shape(false))?,
            motor.config.camera_crouch_ticks,
            motor.capsule_radius,
        )
    };
    let crouched = matches!(previous_mode, MoveState::Crouch | MoveState::Slide { .. });
    let active_height = if crouched {
        config.crouch_height
    } else {
        config.stand_height
    };
    let feet = locomotion.centre - Vec3::Y * (active_height * 0.5);
    let stand_centre = feet + Vec3::Y * (config.stand_height * 0.5);
    let yaw = command.view().yaw();
    let rotation = Quat::from_rotation_y(yaw);
    let can_stand = !lift(world.resource::<PhysicsWorld>()?.intersects_shape(
        standing_collider_shape.clone(),
        stand_centre,
        rotation,
        config.query_filter.excluding_body(locomotion.body),
    ))?;
    let mantle_target =
        if command.jump_pressed() && !matches!(previous_mode, MoveState::Slide { .. }) {
            detect_mantle(
                world.resource::<PhysicsWorld>()?,
                &locomotion,
                &config,
                rotation,
                capsule_radius,
            )?
        } else {
            None
        };
    let facts = TransitionFacts {
        grounded: locomotion.grounded,
        horizontal_speed: state.horizontal.length(),
        can_stand,
        mantle_target,
        mantle_from: locomotion.centre,
        slide_ticks: config.slide_ticks,
        slide_min_speed: config.slide_min_speed,
        mantle_ticks: config.mantle_ticks,
    };
    let next_mode = next_state(previous_mode, command, facts);
    let next_crouched = matches!(next_mode, MoveState::Crouch | MoveState::Slide { .. });
    let mut centre = locomotion.centre;
    if next_crouched != crouched {
        let shape = if next_crouched {
            collider_shape
        } else {
            standing_collider_shape.clone()
        };
        lift(
            world
                .resource_mut::<PhysicsWorld>()?
                .set_collider_shape(locomotion.collider, shape),
        )?;
        centre = lift(
            world
                .resource::<PhysicsWorld>()?
                .body_state(locomotion.body),
        )?
        .translation;
    }
    if mode_changed(previous_mode, next_mode) {
        world.resource_mut::<MovementLog>()?.push(MovementEvent {
            actor: id,
            from: previous_mode,
            to: next_mode,
        });
    }
    if matches!(next_mode, MoveState::Slide { .. })
        && !matches!(previous_mode, MoveState::Slide { .. })
    {
        state.horizontal += state.horizontal.normalize_or_zero() * config.slide_boost;
    }
    let mut new_eye = locomotion.eye_above_centre;
    let eye_stand = (config.stand_height * 0.5 - EYE_CLEARANCE_M).max(0.0);
    let eye_crouch = (config.crouch_height * 0.5 - EYE_CLEARANCE_M).max(0.0);
    let eye_target = if next_crouched { eye_crouch } else { eye_stand };
    new_eye = move_towards_scalar(
        new_eye,
        eye_target,
        (eye_stand - eye_crouch).abs() / f32::from(crouch_ticks),
    );
    if let MoveState::Mantle {
        from,
        to,
        tick,
        ticks,
    } = next_mode
    {
        let elapsed = tick.min(ticks);
        let t = f32::from(elapsed) / f32::from(ticks);
        let eased = t * t * (3.0 - 2.0 * t);
        centre = from.lerp(to, eased);
        state.vertical = 0.0;
    } else if matches!(previous_mode, MoveState::Mantle { .. }) && next_mode == MoveState::Walk {
        if let MoveState::Mantle { to, .. } = previous_mode {
            centre = to;
        }
        state.vertical = 0.0;
    }
    let velocity = if matches!(previous_mode, MoveState::Mantle { .. })
        || matches!(next_mode, MoveState::Mantle { .. })
    {
        Vec3::ZERO
    } else {
        step_velocity_in_mode(
            &mut state,
            &config,
            &command,
            locomotion.grounded,
            dt,
            next_mode,
        )
    };
    if matches!(next_mode, MoveState::Slide { .. })
        && locomotion.grounded
        && let Some(hit) = world.resource::<PhysicsWorld>()?.ray_cast(
            lift(RayCast::new(
                feet + Vec3::Y * SKIN_WIDTH_M,
                -Vec3::Y,
                GROUND_SNAP_M + SKIN_WIDTH_M,
                true,
            ))?,
            config.query_filter.excluding_body(locomotion.body),
        )
    {
        let gravity_tangent = Vec3::new(0.0, -config.gravity, 0.0)
            - hit.normal * Vec3::new(0.0, -config.gravity, 0.0).dot(hit.normal);
        state.horizontal += Vec2::new(gravity_tangent.x, gravity_tangent.z) * dt;
    }
    let input = lift(CharacterInput::new(velocity, false))?;
    // The controller subtracts one gravity step from the vertical speed it is given. Adding it
    // back here makes the controller's vertical part exactly `velocity.y * dt`.
    let start = lift(CharacterState::with_motion(
        centre,
        velocity.y + config.gravity * dt,
        false,
    ))?;
    let controller = world.resource::<Motor>()?.controller(next_crouched);
    let movement = controller.move_character(
        start,
        input,
        world.resource::<PhysicsWorld>()?,
        locomotion.body,
    );
    let grounded = movement.state.grounded();
    if grounded && state.vertical < 0.0 {
        state.vertical = 0.0;
    }
    if !matches!(next_mode, MoveState::Mantle { .. }) {
        centre = movement.state.position();
    }
    lift(
        world
            .resource_mut::<PhysicsWorld>()?
            .set_next_kinematic_pose(locomotion.body, centre, Quat::from_rotation_y(yaw)),
    )?;
    {
        let mut stored = world.get_mut::<Locomotion>(id)?;
        stored.state = state;
        stored.move_state = next_mode;
        stored.centre = centre;
        stored.grounded = grounded;
        stored.eye_above_centre = new_eye;
    }
    let mut transform = world.get_mut::<Transform>(id)?;
    transform.set_translation(centre)?;
    transform.set_rotation(Quat::from_rotation_y(yaw))
}

fn mode_changed(from: MoveState, to: MoveState) -> bool {
    !matches!(
        (from, to),
        (MoveState::Walk, MoveState::Walk)
            | (MoveState::Sprint, MoveState::Sprint)
            | (MoveState::Crouch, MoveState::Crouch)
            | (MoveState::Slide { .. }, MoveState::Slide { .. })
            | (MoveState::Air, MoveState::Air)
            | (MoveState::Mantle { .. }, MoveState::Mantle { .. })
    )
}

fn move_towards_scalar(current: f32, target: f32, step: f32) -> f32 {
    if (target - current).abs() <= step {
        target
    } else {
        current + (target - current).signum() * step
    }
}
