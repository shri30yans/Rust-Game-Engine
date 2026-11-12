//! The dirty queue of elements waiting for a component solve, and the release
//! phase that clears a finished component's marks.
//!
//! Invariant: `pending` counts the `true` entries in `queued`, and an element is
//! queued at most once.

use crate::error::at_mut;
use crate::ids::ElementId;
use crate::state::StructureState;

use super::SolveResult;
use super::job::{Job, Progress, Scratch};

impl StructureState {
    /// True while a solve is in progress or has queued work.
    pub(super) fn has_pending_solve(&self) -> bool {
        self.job.is_some() || self.pending > 0
    }

    /// Adds `id` to the solve queue unless it is already queued.
    pub(crate) fn enqueue(&mut self, id: ElementId) -> SolveResult<()> {
        let flag = at_mut(&mut self.queued, id.index())?;
        if !*flag {
            *flag = true;
            self.pending += 1;
            self.queue.push_back(id);
        }
        Ok(())
    }

    /// Clears the queued flag for `id` and reports whether it was set.
    pub(super) fn take_queued(&mut self, id: ElementId) -> SolveResult<bool> {
        let flag = at_mut(&mut self.queued, id.index())?;
        if !*flag {
            return Ok(false);
        }
        *flag = false;
        self.pending = self.pending.saturating_sub(1);
        Ok(true)
    }

    /// Release: clears the per-element marks for the component and drops it from the queue's pending count.
    pub(super) fn release_step(&mut self, job: &mut Job) -> SolveResult<Progress> {
        let Some(&id) = self.scratch.comp.get(job.cursor) else {
            return Ok(Progress::PhaseDone);
        };
        job.cursor += 1;
        clear_marks(&mut self.scratch, id)?;
        let flag = at_mut(&mut self.queued, id.index())?;
        if *flag {
            *flag = false;
            self.pending = self.pending.saturating_sub(1);
        }
        Ok(Progress::Visited)
    }
}

/// Clears the solve marks an element picked up during a component solve.
fn clear_marks(scratch: &mut Scratch, id: ElementId) -> SolveResult<()> {
    let index = id.index();
    *at_mut(&mut scratch.in_job, index)? = false;
    *at_mut(&mut scratch.seen, index)? = false;
    *at_mut(&mut scratch.parent, index)? = None;
    Ok(())
}
