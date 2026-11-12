//! Unit tests for the controller: the pure movement rule, motor retuning and mantle detection.

use std::f32::consts::FRAC_PI_2;

use engine_physics::{ConvexShape, PhysicsWorld};
use glam::{Quat, Vec2, Vec3};

use super::mantle::detect_mantle;
use crate::command::{MoveCommand, Trigger};
use crate::error::KitError;

use app::ViewAngles;
use engine_physics::{BodyBuilder, ColliderBuilder, PhysicsConfig};
use proptest::prelude::*;

use super::*;

const DT: f32 = 1.0 / 60.0;
/// Jump height of the test tuning at full jump: `jump_speed^2 / (2 * gravity)`.
const FULL_JUMP_M: f32 = 8.0 * 8.0 / (2.0 * 20.0);

fn tuning() -> ControllerTuning {
    ControllerTuning {
        walk_speed: 6.0,
        ground_accel: 60.0,
        ground_friction: 40.0,
        air_accel: 10.0,
        air_speed_cap: 7.0,
        jump_speed: 8.0,
        gravity: 20.0,
        max_fall_speed: 30.0,
        coyote_ticks: 3,
        jump_buffer_ticks: 4,
        jump_cut: 0.5,
        step_height: 0.4,
        slope_limit: 0.8,
        sprint_speed: 8.5,
        crouch_speed: 2.5,
        stand_height: 1.8,
        crouch_height: 1.1,
        slide_min_speed: 5.0,
        slide_boost: 1.5,
        slide_friction: 5.0,
        slide_ticks: 42,
        mantle_reach: 0.8,
        mantle_min_height: 0.2,
        mantle_max_height: 1.6,
        mantle_ticks: 42,
        camera_crouch_ticks: 10,
    }
}

fn config() -> Result<ControllerConfig, KitError> {
    ControllerConfig::new(tuning())
}

fn mantle_target(height: f32, slope: f32) -> Result<Option<Vec3>, Box<dyn std::error::Error>> {
    let mut physics = PhysicsWorld::new(PhysicsConfig::new(DT, Vec3::ZERO)?);
    let shape = ConvexShape::cuboid(Vec3::new(1.0, height * 0.5, 0.5))?;
    let ledge = ColliderBuilder::new(shape)
        .translation(Vec3::new(0.0, height * 0.5, -1.0))?
        .rotation(Quat::from_rotation_x(slope))?;
    physics.insert_collider(None, ledge)?;
    let body = physics.insert_body(BodyBuilder::kinematic().translation(Vec3::new(0.0, 0.9, 0.0))?);
    let collider = physics.insert_collider(
        Some(body),
        ColliderBuilder::new(ConvexShape::capsule(0.6, 0.3)?),
    )?;
    let locomotion = Locomotion::new(body, collider, Vec3::new(0.0, 0.9, 0.0), 0.8)?;
    Ok(detect_mantle(
        &physics,
        &locomotion,
        &config()?,
        Quat::IDENTITY,
        0.3,
    )?)
}

#[test]
fn mantle_probe_accepts_supported_ledge_heights_and_rejects_overheight()
-> Result<(), Box<dyn std::error::Error>> {
    for height in [0.8, 1.2, 1.6] {
        assert!(
            mantle_target(height, 0.0)?.is_some(),
            "expected {height} m mantle"
        );
    }
    assert!(mantle_target(1.7, 0.0)?.is_none());
    assert!(mantle_target(1.2, 60.0_f32.to_radians())?.is_none());
    Ok(())
}

#[test]
fn retuning_replaces_movement_config_without_changing_capsule_setup() -> Result<(), KitError> {
    let first = config()?;
    let mut motor = Motor::new(first, 0.3)?;
    let replacement = ControllerConfig::new(ControllerTuning {
        walk_speed: 9.0,
        sprint_speed: 11.0,
        ..tuning()
    })?;
    motor.set_config(replacement)?;
    assert_eq!(motor.config(), &replacement);
    assert_eq!(motor.config().walk_speed(), 9.0);
    Ok(())
}

proptest! {
    #[test]
    fn retuning_generated_valid_controller_configs_keeps_a_valid_config(
        walk_speed in 3.0f32..30.0,
        jump_speed in 0.1f32..30.0,
    ) {
        let first = config().map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut motor = Motor::new(first, 0.3)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let replacement = ControllerConfig::new(ControllerTuning {
            walk_speed,
            sprint_speed: walk_speed + 1.0,
            jump_speed,
            ..tuning()
        }).map_err(|error| TestCaseError::fail(error.to_string()))?;
        motor.set_config(replacement)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(motor.config(), &replacement);
        prop_assert!(motor.config().walk_speed().is_finite());
        prop_assert!(motor.config().jump_speed().is_finite());
    }
}

/// A command with the given move axis, facing yaw zero, and the given jump buttons.
fn command(axis: Vec2, pressed: bool, held: bool) -> Result<MoveCommand, KitError> {
    MoveCommand::new(
        axis,
        ViewAngles::ZERO,
        pressed,
        held,
        Trigger::Released,
        false,
    )
}

/// Peak height of a jump launched on the first tick, with the button held or tapped.
fn peak_height(held: bool) -> Result<f32, KitError> {
    let cfg = config()?;
    let mut state = LocomotionState::default();
    let press = command(Vec2::ZERO, true, held)?;
    let airborne = command(Vec2::ZERO, false, held)?;
    let mut height = 0.0_f32;
    let first = step_velocity(&mut state, &cfg, &press, true, DT);
    height += first.y * DT;
    let mut peak = height;
    for _ in 0..600 {
        let velocity = step_velocity(&mut state, &cfg, &airborne, false, DT);
        height += velocity.y * DT;
        peak = peak.max(height);
    }
    Ok(peak)
}

#[test]
fn invalid_tuning_is_rejected() {
    let mut bad = tuning();
    bad.jump_cut = 1.5;
    assert!(ControllerConfig::new(bad).is_err());
    let mut bad = tuning();
    bad.slope_limit = FRAC_PI_2;
    assert!(ControllerConfig::new(bad).is_err());
    let mut bad = tuning();
    bad.walk_speed = f32::NAN;
    assert!(ControllerConfig::new(bad).is_err());
}

#[test]
fn ground_speed_reaches_walk_speed_and_no_more() -> Result<(), KitError> {
    let cfg = config()?;
    let forward = command(Vec2::new(0.0, 1.0), false, false)?;
    let mut state = LocomotionState::default();
    for _ in 0..600 {
        step_velocity(&mut state, &cfg, &forward, true, DT);
        assert!(state.horizontal().length() <= 6.0 + 1e-4);
    }
    assert!((state.horizontal().length() - 6.0).abs() < 1e-4);
    // Forward at yaw zero is -Z, as the sandbox defines it.
    assert!((state.horizontal().y + 6.0).abs() < 1e-4);
    Ok(())
}

#[test]
fn friction_brings_ground_speed_to_rest() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let mut state = LocomotionState {
        horizontal: Vec2::new(6.0, 0.0),
        ..LocomotionState::default()
    };
    for _ in 0..30 {
        step_velocity(&mut state, &cfg, &idle, true, DT);
    }
    assert_eq!(state.horizontal(), Vec2::ZERO);
    Ok(())
}

#[test]
fn held_jump_reaches_v_squared_over_two_g() -> Result<(), KitError> {
    let peak = peak_height(true)?;
    assert!(
        (peak - FULL_JUMP_M).abs() <= FULL_JUMP_M * 0.01,
        "peak {peak} m, expected {FULL_JUMP_M} m"
    );
    Ok(())
}

#[test]
fn tapped_jump_reaches_cut_squared_of_full_height() -> Result<(), KitError> {
    let expected = FULL_JUMP_M * 0.5 * 0.5;
    let peak = peak_height(false)?;
    assert!(
        (peak - expected).abs() <= expected * 0.01,
        "peak {peak} m, expected {expected} m"
    );
    Ok(())
}

#[test]
fn coyote_time_allows_a_jump_within_its_window() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let press = command(Vec2::ZERO, true, true)?;
    let mut state = LocomotionState::default();
    step_velocity(&mut state, &cfg, &idle, true, DT);
    // Two airborne ticks leave one tick of coyote time.
    for _ in 0..2 {
        step_velocity(&mut state, &cfg, &idle, false, DT);
    }
    step_velocity(&mut state, &cfg, &press, false, DT);
    assert!(state.vertical() > 7.0, "vertical {}", state.vertical());
    Ok(())
}

#[test]
fn coyote_time_expires() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let press = command(Vec2::ZERO, true, true)?;
    let mut state = LocomotionState::default();
    step_velocity(&mut state, &cfg, &idle, true, DT);
    // Three airborne ticks use up the three ticks of coyote time.
    for _ in 0..3 {
        step_velocity(&mut state, &cfg, &idle, false, DT);
    }
    step_velocity(&mut state, &cfg, &press, false, DT);
    assert!(state.vertical() < 0.0, "vertical {}", state.vertical());
    Ok(())
}

#[test]
fn a_press_just_before_landing_jumps_on_landing() -> Result<(), KitError> {
    let cfg = config()?;
    let press = command(Vec2::ZERO, true, true)?;
    let mut state = LocomotionState::default();
    step_velocity(&mut state, &cfg, &press, false, DT);
    // The button stays held on landing, so the buffered jump is not cut.
    let held_on_landing = command(Vec2::ZERO, false, true)?;
    step_velocity(&mut state, &cfg, &held_on_landing, true, DT);
    assert!(state.vertical() > 7.0, "vertical {}", state.vertical());
    Ok(())
}

#[test]
fn a_press_older_than_the_buffer_does_not_jump_on_landing() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let press = command(Vec2::ZERO, true, true)?;
    let mut state = LocomotionState::default();
    step_velocity(&mut state, &cfg, &press, false, DT);
    // Four ticks of buffer: the press is forgotten after the fourth airborne tick.
    for _ in 0..4 {
        step_velocity(&mut state, &cfg, &idle, false, DT);
    }
    step_velocity(&mut state, &cfg, &idle, true, DT);
    assert!(state.vertical() < 0.0, "vertical {}", state.vertical());
    Ok(())
}

#[test]
fn air_speed_is_capped() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let mut state = LocomotionState {
        horizontal: Vec2::new(10.0, 0.0),
        ..LocomotionState::default()
    };
    step_velocity(&mut state, &cfg, &idle, false, DT);
    assert!(state.horizontal().length() <= 7.0 + 1e-4);
    Ok(())
}

#[test]
fn falling_speed_is_capped_at_terminal_velocity() -> Result<(), KitError> {
    let cfg = config()?;
    let idle = command(Vec2::ZERO, false, false)?;
    let mut state = LocomotionState::default();
    for _ in 0..300 {
        step_velocity(&mut state, &cfg, &idle, false, DT);
    }
    assert!(
        (state.vertical() + 30.0).abs() < 1e-4,
        "vertical {}",
        state.vertical()
    );
    Ok(())
}

proptest! {
    #[test]
    fn speeds_stay_within_the_configured_bounds(
        steps in proptest::collection::vec(
            (
                -1.0f32..=1.0,
                -1.0f32..=1.0,
                -3.0f32..=3.0,
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
                0.01f32..=0.05,
            ),
            1..200,
        ),
    ) {
        let cfg = config().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut state = LocomotionState::default();
        for (x, y, yaw, pressed, held, on_ground, dt) in steps {
            let view = ViewAngles::new(yaw, 0.0).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let cmd = MoveCommand::new(
                Vec2::new(x, y),
                view,
                pressed,
                held,
                Trigger::Released,
                false,
            )
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            step_velocity(&mut state, &cfg, &cmd, on_ground, dt);
            prop_assert!(state.horizontal().length() <= cfg.walk_speed().max(cfg.air_speed_cap()) + 1e-3);
            prop_assert!(state.vertical() <= cfg.jump_speed() + 1e-3);
            prop_assert!(state.vertical() >= -cfg.max_fall_speed() - 1e-3);
        }
    }
}
