//! Validated convex, triangle-mesh and compound shapes plus collider metadata.
//!
//! Shape geometry is held by Rapier's shared shape handles, so cloning a collider shape does not
//! duplicate cooked vertex data.

mod builder;
mod compound;
mod convex;
mod decompose;
mod id;
mod layers;
mod shape;
mod trimesh;

pub use builder::ColliderBuilder;
pub use compound::CompoundShape;
pub use convex::ConvexShape;
pub use decompose::{DecompositionParams, decompose_mesh};
pub use id::{ColliderId, ColliderOwner};
pub use layers::{CollisionLayers, LayerMask, SurfaceId};
pub use shape::ColliderShape;
pub use trimesh::TriMeshShape;
