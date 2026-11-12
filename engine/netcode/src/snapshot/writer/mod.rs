//! Per-client dirty state selection, packet assembly, and acknowledgement shadow.
//!
//! `state` owns the writer's storage, `tracked` the per-entity acknowledged shadow, `refresh`
//! and `diff` bring that shadow up to date with each write, `assemble` fills one packet within
//! the `size` budget, `ack` records sent packets and applies acknowledgements, `queue` holds
//! game-queued despawns and events, and `policy` is the game's per-entity input.

mod ack;
mod assemble;
mod diff;
mod policy;
mod queue;
mod refresh;
mod size;
mod state;
mod tracked;

pub use policy::EntityReplication;
pub use size::encoded_size;
pub use state::SnapshotWriter;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
