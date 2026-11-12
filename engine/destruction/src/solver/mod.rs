//! Budgeted, resumable load solver and the collapse countdown.
//!
//! Model: an element is supported when a path of live elements reaches an
//! anchor. Within one connected component of live elements, either every
//! member is supported (the component holds an anchor) or none is, so a
//! component without an anchor detaches as one group. Loads come from a
//! breadth-first support tree rooted at the anchors, visited in ascending id
//! order, with each element's parent being the first element to discover it.
//! An element's load is its own mass plus the loads of its children. An
//! `Intact` element whose load exceeds its effective capacity becomes `Failing`.
//!
//! Scheduling invariants:
//! - A cycle starts at a call to [`StructureState::step`] from a cycle boundary.
//!   That call applies the waiting damage batch before any queued solve, including
//!   the initial full-structure solve. A call that finishes a cycle does not start
//!   another one, leaving a boundary at which callers may submit more input.
//! - A solve job is resumable: each element processed costs one visit, and the
//!   job keeps its phase and cursor between steps.
//! - Countdown rounds run only once every queued solve has finished. Each round
//!   removes one warning tick from every `Failing` element, and a step runs at
//!   most one round's worth of work of either kind.
//! - Damage submitted during an active cycle waits for the next boundary. Budgets
//!   change when work finishes, never which cycle consumes input or what it finds.
//!
//! Modules (private; the public surface is `StructureState::step`):
//! - `job`: the solve phases, the resumable job, its scratch space and outcomes.
//! - `driver`: the cycle driver behind `StructureState::step`.
//! - `queue`: the dirty queue and the release phase.
//! - `connectivity`: component collection and the support tree.
//! - `load`: load propagation and overload detection.
//! - `apply`: applying outcomes and recording detached groups.
//! - `countdown`: the collapse countdown.

mod apply;
mod connectivity;
mod countdown;
mod driver;
mod job;
mod load;
mod queue;

pub(crate) use job::{Job, Scratch};

use crate::error::DestructionError;

type SolveResult<T> = Result<T, DestructionError>;
