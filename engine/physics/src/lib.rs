//! Deterministic Rapier physics wrapper and shared kinematic character controller.
//!
//! The world advances only by its validated fixed timestep and never reads a clock.
//! Rigid-body and collider handles stay private behind generation-checked identifiers.
//! Stable-topology steps reuse Rapier's internal storage after warm-up; insertion,
//! removal, new contact graph capacity, and dependency-internal growth may allocate
//! and are measured by the dedicated counting-allocator integration test.
#![forbid(unsafe_code)]

#[cfg(all(feature = "cross-platform-determinism", feature = "simd"))]
compile_error!(
    "cross-platform-determinism and simd are exclusive: SIMD breaks cross-platform bit identity."
);

mod body;
mod character;
mod collider;
mod config;
mod convert;
mod error;
mod query;
mod world;

pub use body::{BodyBuilder, BodyId, BodyKind, BodyState};
pub use character::{
    CharacterConfig, CharacterController, CharacterInput, CharacterMovement, CharacterState,
};
pub use collider::{
    ColliderBuilder, ColliderId, ColliderOwner, ColliderShape, CollisionLayers, CompoundShape,
    ConvexShape, DecompositionParams, LayerMask, SurfaceId, TriMeshShape, decompose_mesh,
};
pub use config::PhysicsConfig;
pub use error::PhysicsError;
pub use query::{QueryFilter, RayCast, RayCastHit, ShapeCast, ShapeCastHit};
pub use world::PhysicsWorld;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
