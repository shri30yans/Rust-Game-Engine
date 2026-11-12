//! Engine world: a thin, API-restricted wrapper over `hecs`.
//!
//! Modules:
//! - [`net_id`]: stable, never-reused [`NetId`]s; hecs entities never leave this crate.
//! - [`transform`]: validated [`Transform`], derived [`GlobalTransform`] and [`propagate_transforms`].
//! - [`hierarchy`]: parent/child links, editable only through `World` methods, cycle-free by construction.
//! - [`registry`]: [`ComponentRegistry`] with replication classes and postcard codecs.
//! - [`resource`]: [`Resource`] singleton state (physics world, match clock) owned by the world and covered by its state hash.
//! - [`world`]: the [`World`] that owns all of the above plus the root RNG and state hash.
//! - [`schedule`]: named, ordered systems with a per-system RNG stream and a fixed timestep.
//!
//! Invariants: no wall-clock reads, no OS entropy, no global mutable state, no
//! panics or unchecked indexing in non-test code. The hierarchy is a forest
//! because only `set_parent` writes links and it rejects loops before writing.
#![forbid(unsafe_code)]

pub mod error;
mod hierarchy;
pub mod net_id;
pub mod registry;
mod resource;
pub mod schedule;
pub mod transform;
pub mod world;

pub use error::WorldError;
pub use hecs::Component;
pub use net_id::NetId;
pub use registry::{ComponentId, ComponentRegistry, Replication};
pub use resource::Resource;
pub use schedule::{Schedule, SystemContext, SystemFn};
pub use transform::{GlobalTransform, Transform, propagate_transforms};
pub use world::World;
