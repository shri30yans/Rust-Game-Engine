//! Typed errors for kit-fps. Configs and commands are validated when built, so every error here
//! names the field that was rejected. Failures from the engine crates keep their source.
//!
//! Systems return `WorldError`, so the kit lifts its own failures through
//! `WorldError::Downstream` with [`lift`], which keeps the original error as the source.

use crate::aim_assist::AimAssistError;
use app::AppError;
use engine_core::CoreError;
use engine_physics::PhysicsError;
use engine_world::WorldError;
use thiserror::Error;

/// Errors produced by kit-fps.
#[derive(Debug, Error)]
pub enum KitError {
    /// A scalar is NaN or infinite.
    #[error("{field} must be finite, got {value}")]
    NonFinite {
        /// Name of the rejected field.
        field: &'static str,
        /// Rejected value.
        value: f32,
    },
    /// A vector has a non-finite component.
    #[error("{field} must have only finite components")]
    NonFiniteVector {
        /// Name of the rejected field.
        field: &'static str,
    },
    /// A scalar lies outside its accepted range.
    #[error("{field} must be {requirement}, got {value}")]
    OutOfRange {
        /// Name of the rejected field.
        field: &'static str,
        /// Human-readable requirement.
        requirement: &'static str,
        /// Rejected value.
        value: f32,
    },
    /// An integer count lies outside its accepted range.
    #[error("{field} must be {requirement}, got {value}")]
    CountOutOfRange {
        /// Name of the rejected field.
        field: &'static str,
        /// Human-readable requirement.
        requirement: &'static str,
        /// Rejected value.
        value: u32,
    },
    /// Bots exist but the game gave no waypoints for them to patrol.
    #[error("bots exist but the waypoint list is empty")]
    NoWaypoints,
    /// The waypoint link graph is not connected: this waypoint cannot be reached from the first.
    #[error("waypoint {waypoint} cannot be reached from waypoint 0 through the links")]
    Disconnected {
        /// Index of the unreachable waypoint in the list.
        waypoint: usize,
    },
    /// More waypoints were given than a waypoint identifier can name.
    #[error("{count} waypoints given, at most {max} are allowed")]
    TooManyWaypoints {
        /// Number of waypoints given.
        count: usize,
        /// Largest number of waypoints allowed.
        max: usize,
    },
    /// The physics engine rejected a value or a query.
    #[error(transparent)]
    Physics(#[from] PhysicsError),
    /// The world rejected an operation.
    #[error(transparent)]
    World(#[from] WorldError),
    /// The engine core rejected an operation.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// The engine app rejected a view angle.
    #[error(transparent)]
    App(#[from] AppError),
    /// Aim slowdown parameters failed validation.
    #[error(transparent)]
    AimAssist(#[from] AimAssistError),
}

/// Converts a kit or engine error into the world's error type for a system. A world error passes
/// through unchanged; every other error is kept whole as the source of `WorldError::Downstream`.
pub fn lift<T, E>(result: Result<T, E>) -> Result<T, WorldError>
where
    E: Into<LiftedError>,
{
    result.map_err(|error| match error.into() {
        LiftedError::World(world) => world,
        LiftedError::Other(source) => WorldError::Downstream { source },
    })
}

/// The two shapes a lifted error can take. Kept private: callers only use [`lift`].
pub enum LiftedError {
    /// An error that is already a world error.
    World(WorldError),
    /// Any other error from a crate above the world.
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl From<WorldError> for LiftedError {
    fn from(error: WorldError) -> Self {
        Self::World(error)
    }
}

impl From<KitError> for LiftedError {
    fn from(error: KitError) -> Self {
        match error {
            KitError::World(world) => Self::World(world),
            other => Self::Other(Box::new(other)),
        }
    }
}

impl From<PhysicsError> for LiftedError {
    fn from(error: PhysicsError) -> Self {
        Self::Other(Box::new(error))
    }
}

impl From<AppError> for LiftedError {
    fn from(error: AppError) -> Self {
        Self::Other(Box::new(error))
    }
}
