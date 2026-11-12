//! Connectivity: collects the live component around a seed element, and builds
//! the breadth-first support tree from the component's anchors.
//!
//! Invariant: a component with no anchor detaches as one group, and the support
//! order covers the whole component when the support phase finishes.

use std::sync::Arc;

use crate::error::{DestructionError, at, at_mut};
use crate::state::StructureState;

use super::SolveResult;
use super::job::{Job, Phase, Progress};

impl StructureState {
    /// True when any member of the collected component is an anchor.
    pub(super) fn component_has_anchor(&self) -> bool {
        self.scratch.comp.iter().any(|&id| {
            self.structure
                .element(id)
                .is_some_and(|def| def.is_anchor())
        })
    }

    /// Enters the support phase, seeding the breadth-first search with the component's anchors.
    pub(super) fn begin_support(&mut self, job: &mut Job) -> SolveResult<()> {
        job.phase = Phase::Support;
        job.cursor = 0;
        self.scratch.frontier.clear();
        for index in 0..self.scratch.comp.len() {
            let id = *at(&self.scratch.comp, index)?;
            if self
                .structure
                .element(id)
                .is_some_and(|def| def.is_anchor())
            {
                *at_mut(&mut self.scratch.seen, id.index())? = true;
                self.scratch.frontier.push_back(id);
            }
        }
        Ok(())
    }

    /// Collect: pops one element and adds its live, unvisited neighbours to the component.
    pub(super) fn collect_step(&mut self) -> SolveResult<Progress> {
        let Some(current) = self.scratch.frontier.pop_front() else {
            return Ok(Progress::PhaseDone);
        };
        let mass = self
            .structure
            .element(current)
            .ok_or(DestructionError::Invariant("job element missing"))?
            .mass();
        *at_mut(&mut self.scratch.load, current.index())? = mass;
        let structure = Arc::clone(&self.structure);
        for &next in structure.neighbours(current) {
            if self.is_live(next) && !*at(&self.scratch.in_job, next.index())? {
                *at_mut(&mut self.scratch.in_job, next.index())? = true;
                self.scratch.comp.push(next);
                self.scratch.frontier.push_back(next);
            }
        }
        Ok(Progress::Visited)
    }

    /// Support: pops one element in breadth-first order and claims its unclaimed neighbours as children.
    pub(super) fn support_step(&mut self) -> SolveResult<Progress> {
        let Some(current) = self.scratch.frontier.pop_front() else {
            return Ok(Progress::PhaseDone);
        };
        self.scratch.order.push(current);
        let structure = Arc::clone(&self.structure);
        for &next in structure.neighbours(current) {
            let index = next.index();
            if *at(&self.scratch.in_job, index)? && !*at(&self.scratch.seen, index)? {
                *at_mut(&mut self.scratch.seen, index)? = true;
                *at_mut(&mut self.scratch.parent, index)? = Some(current);
                self.scratch.frontier.push_back(next);
            }
        }
        Ok(Progress::Visited)
    }
}
