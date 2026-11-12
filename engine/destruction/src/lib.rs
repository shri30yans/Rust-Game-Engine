//! Engine destruction: the structural core of destruction. No rendering, no physics.
//!
//! Modules:
//! - [`ids`]: typed element, group and material ids.
//! - [`material`]: validated physical constants that elements derive from.
//! - [`element`]: element specs, immutable definitions, life-cycle states and health bands.
//! - [`builder`]: the only way to make a [`structure::Structure`]; it validates everything.
//! - [`structure`]: immutable topology in CSR form with sorted neighbour lists.
//! - [`state`]: the mutable [`state::StructureState`], its damage, rounds and state hash.
//! - [`solver`]: the budgeted, resumable load solver and its dirty queue.
//! - [`budget`]: the per-step element-visit budget.
//! - [`events`]: the reusable event sink.
//! - [`blast`]: sphere blasts with distance falloff.
//! - [`error`]: typed errors.
//!
//! Invariants: no wall-clock reads, no OS entropy, no global mutable state, no
//! panics and no unchecked indexing outside tests. Damage is queued and applies
//! only at solve-cycle boundaries, in ascending element-id order. Damage submitted
//! during a solve or countdown waits until that cycle settles. The same structure
//! and cycle-boundary inputs produce the same settled state hash for every budget.
#![forbid(unsafe_code)]

pub mod blast;
pub mod budget;
pub mod builder;
pub mod element;
pub mod error;
pub mod events;
pub mod ids;
pub mod material;
pub mod solver;
pub mod state;
pub mod structure;

pub use blast::{Blast, Falloff};
pub use budget::DestructionBudget;
pub use builder::StructureBuilder;
pub use element::{ElementDef, ElementSpec, ElementState, HealthBand};
pub use error::DestructionError;
pub use events::{DestructionEvent, Event, EventSink};
pub use ids::{ElementId, GroupId, MaterialId};
pub use material::Material;
pub use state::{DetachedGroup, MassTotals, StepReport, StructureState};
pub use structure::{DestructionConfig, Structure};
