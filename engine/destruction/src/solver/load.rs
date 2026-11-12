//! Load propagation and overload detection. Loads flow from the leaves of the
//! support tree up to its roots. Each intact element whose load exceeds its
//! effective capacity is recorded as overloaded.
//!
//! Invariant: effective capacity scales with current health, so a damaged element
//! carries less load before it overloads.

use crate::element::ElementState;
use crate::error::{DestructionError, at, at_mut};
use crate::state::StructureState;

use super::SolveResult;
use super::job::{Decision, Job, Outcome, Progress};

impl StructureState {
    /// Load: walks the support order from the leaves up, passing each element's load to its parent.
    pub(super) fn load_step(&mut self, job: &mut Job) -> SolveResult<Progress> {
        let len = self.scratch.order.len();
        let Some(position) = len.checked_sub(job.cursor + 1) else {
            return Ok(Progress::PhaseDone);
        };
        let id = *at(&self.scratch.order, position)?;
        job.cursor += 1;
        if let Some(parent) = *at(&self.scratch.parent, id.index())? {
            let carried = *at(&self.scratch.load, id.index())?;
            *at_mut(&mut self.scratch.load, parent.index())? += carried;
        }
        Ok(Progress::Visited)
    }

    /// Decide: records every `Intact` element whose load exceeds its effective capacity.
    pub(super) fn decide_step(&mut self, job: &mut Job) -> SolveResult<Progress> {
        let Some(&id) = self.scratch.order.get(job.cursor) else {
            return Ok(Progress::PhaseDone);
        };
        job.cursor += 1;
        let index = id.index();
        if *at(&self.states, index)? == ElementState::Intact {
            let def = self
                .structure
                .element(id)
                .ok_or(DestructionError::Invariant("job element missing"))?;
            let health = f32::from(*at(&self.health, index)?);
            let max = f32::from(def.max_health().get());
            let capacity = def.capacity() * health / max;
            if *at(&self.scratch.load, index)? > capacity {
                self.scratch.decisions.push(Decision {
                    id,
                    outcome: Outcome::Overload,
                });
            }
        }
        Ok(Progress::Visited)
    }
}
