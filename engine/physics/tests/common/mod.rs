//! Shared test setup for characters with their own kinematic capsule body.

use engine_physics::{
    BodyBuilder, BodyId, CharacterConfig, CharacterController, CharacterState, ColliderBuilder,
    PhysicsWorld,
};
use glam::Vec3;

/// Adds a kinematic capsule and returns its controller, body id, and initial state.
pub fn spawn_character(
    world: &mut PhysicsWorld,
    config: CharacterConfig,
    position: Vec3,
) -> (CharacterController, BodyId, CharacterState) {
    let body = world.insert_body(BodyBuilder::kinematic().translation(position).unwrap());
    world
        .insert_collider(Some(body), ColliderBuilder::new(config.shape()))
        .unwrap();
    (
        CharacterController::new(config),
        body,
        CharacterState::new(position).unwrap(),
    )
}
