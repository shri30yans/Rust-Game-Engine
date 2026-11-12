//! Shared kinematic character movement built on Rapier's character controller. Every move names
//! the moving body's ID so all movement queries can exclude its attached colliders.
//!
//! All evolving state and input are explicit validated values. Calling
//! [`CharacterController::move_character`] with equal state, input, world, and fixed
//! configuration produces equal output on client and server.

mod config;
mod controller;
mod ground;
mod input;
mod state;

pub use config::CharacterConfig;
pub use controller::CharacterController;
pub use input::CharacterInput;
pub use state::{CharacterMovement, CharacterState};
