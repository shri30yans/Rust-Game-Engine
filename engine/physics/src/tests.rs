//! Crate-level unit tests for validation invariants.

use crate::{CharacterConfig, CharacterInput, ConvexShape, PhysicsConfig};
use glam::Vec3;

#[test]
fn fixed_step_configuration_validates_boundaries() {
    assert!(PhysicsConfig::new(f32::NAN, Vec3::ZERO).is_err());
    assert!(PhysicsConfig::new(-1.0, Vec3::ZERO).is_err());
    let config = PhysicsConfig::new(1.0 / 120.0, Vec3::new(0.0, -9.81, 0.0)).unwrap();
    assert_eq!(config.dt(), 1.0 / 120.0);
}

#[test]
fn shape_constructors_reject_degenerate_geometry() {
    assert!(ConvexShape::ball(0.0).is_err());
    assert!(ConvexShape::cuboid(Vec3::new(1.0, 0.0, 1.0)).is_err());
    assert!(ConvexShape::capsule(-0.1, 0.5).is_err());
    assert!(ConvexShape::capsule(0.0, 0.5).is_ok());
}

#[test]
fn character_values_preserve_their_invariants() {
    let input = CharacterInput::new(Vec3::new(1.0, 99.0, 2.0), false).unwrap();
    assert_eq!(input.ground_velocity(), Vec3::new(1.0, 0.0, 2.0));
    let config = CharacterConfig::capsule(0.5, 0.4).unwrap();
    assert!(config.slope_limit(0.0).is_err());
    assert!(config.slope_limit(core::f32::consts::FRAC_PI_2).is_err());
}
