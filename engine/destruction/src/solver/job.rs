//! Solve job state: the phases of one component solve, its resumable cursor,
//! the per-element scratch buffers, and the outcomes a solve records.
//!
//! Invariant: between jobs every `in_job` and `seen` flag is false again. The
//! release phase clears the flags of the component it collected.

use std::collections::VecDeque;

use crate::ids::ElementId;

/// What a solve decided for one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The element is overloaded and starts its warning phase.
    Overload,
    /// The element lost every support path and falls with its group.
    Detach,
}

/// One element's pending outcome, recorded during the decide phase and applied later.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decision {
    pub(super) id: ElementId,
    pub(super) outcome: Outcome,
}

/// Phases of one component solve, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Gather the live component around the seed and reset its loads.
    Collect,
    /// Build the support tree breadth-first from the component's anchors.
    Support,
    /// Accumulate subtree loads from leaves up to the roots.
    Load,
    /// Record which elements are overloaded.
    Decide,
    /// Apply the recorded outcomes and record any detached group.
    Apply,
    /// Clear the per-component marks so the scratch space is clean.
    Release,
}

/// A resumable component solve, kept between steps when the budget runs out.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Job {
    pub(crate) phase: Phase,
    pub(crate) cursor: usize,
}

/// Per-element scratch space, sized once so steady-state steps never allocate.
#[derive(Debug, Clone)]
pub(crate) struct Scratch {
    pub(super) in_job: Vec<bool>,
    pub(super) seen: Vec<bool>,
    pub(super) parent: Vec<Option<ElementId>>,
    pub(super) load: Vec<f32>,
    pub(super) comp: Vec<ElementId>,
    pub(super) frontier: VecDeque<ElementId>,
    pub(super) order: Vec<ElementId>,
    pub(super) decisions: Vec<Decision>,
}

impl Scratch {
    /// Scratch space for a structure of `n` elements.
    pub(crate) fn new(n: usize) -> Self {
        Self {
            in_job: vec![false; n],
            seen: vec![false; n],
            parent: vec![None; n],
            load: vec![0.0; n],
            comp: Vec::with_capacity(n),
            frontier: VecDeque::with_capacity(n),
            order: Vec::with_capacity(n),
            decisions: Vec::with_capacity(n),
        }
    }
}

/// Result of one phase step: either one element was processed, or the phase is over.
pub(super) enum Progress {
    Visited,
    PhaseDone,
}
