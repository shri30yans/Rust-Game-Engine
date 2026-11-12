//! The cycle driver behind `StructureState::step`. At a cycle boundary it applies
//! the pending damage batch. It then spends the step budget on queued component
//! solves, or on one countdown round, and keeps a job between steps when the
//! budget runs out. Each phase is run by its own module; this file only moves
//! between phases.
//!
//! Invariant: a job ends only through its release phase, which clears the scratch
//! marks before the next job starts.

use engine_core::Tick;

use crate::budget::DestructionBudget;
use crate::error::{DestructionError, at_mut};
use crate::events::EventSink;
use crate::ids::ElementId;
use crate::state::{CycleState, StepReport, StructureState};

use super::SolveResult;
use super::job::{Decision, Job, Outcome, Phase, Progress};

impl StructureState {
    /// Advances the current solve cycle by one budgeted step.
    ///
    /// At a cycle boundary, this first applies the pending damage batch in ascending
    /// element-id order. The step then spends at most `budget` element visits on
    /// queued solves or one countdown round. Work that does not fit resumes on the
    /// next call. A call that completes a cycle leaves the state at its boundary;
    /// damage queued during that cycle can start only on a later call.
    ///
    /// # Errors
    /// Returns [`DestructionError::Invariant`] only if an internal table is out of step.
    pub fn step(
        &mut self,
        tick: Tick,
        sink: &mut EventSink,
        budget: DestructionBudget,
    ) -> SolveResult<StepReport> {
        if self.cycle == CycleState::Boundary {
            self.apply_pending_damage(sink)?;
            if !self.cycle_work_complete() {
                self.cycle = CycleState::Active;
            }
        }

        let mut left = usize::try_from(budget.max_element_visits().get()).unwrap_or(usize::MAX);
        let mut visits = 0usize;
        if self.cycle == CycleState::Active {
            if self.countdown.is_some() {
                self.run_countdown(tick, sink, &mut left, &mut visits)?;
            } else if self.has_pending_solve() {
                self.run_solve(tick, sink, &mut left, &mut visits)?;
            } else if !self.failing.is_empty() {
                self.countdown = Some(0);
                self.run_countdown(tick, sink, &mut left, &mut visits)?;
            }

            if self.cycle_work_complete() {
                self.cycle = CycleState::Boundary;
            }
        }

        Ok(StepReport {
            visits,
            settled: self.is_settled(),
        })
    }

    /// Runs queued solves until the budget is spent or the queue is empty.
    fn run_solve(
        &mut self,
        tick: Tick,
        sink: &mut EventSink,
        left: &mut usize,
        visits: &mut usize,
    ) -> SolveResult<()> {
        loop {
            if self.job.is_none() {
                let Some(seed) = self.queue.pop_front() else {
                    return Ok(());
                };
                if !self.take_queued(seed)? || !self.is_live(seed) {
                    continue;
                }
                self.job = Some(self.start_job(seed)?);
            }
            if *left == 0 {
                return Ok(());
            }
            if !self.advance_job(tick, sink, left, visits)? {
                return Ok(());
            }
        }
    }

    /// Runs the active job until it finishes or the budget is spent. Returns true when it finished.
    fn advance_job(
        &mut self,
        tick: Tick,
        sink: &mut EventSink,
        left: &mut usize,
        visits: &mut usize,
    ) -> SolveResult<bool> {
        let Some(mut job) = self.job.take() else {
            return Ok(true);
        };
        let finished = loop {
            if *left == 0 {
                break false;
            }
            match self.step_phase(&mut job, tick, sink)? {
                Progress::Visited => {
                    *left -= 1;
                    *visits += 1;
                }
                Progress::PhaseDone => {
                    if self.finish_phase(&mut job, tick, sink)? {
                        break true;
                    }
                }
            }
        };
        if finished {
            self.reset_scratch();
        } else {
            self.job = Some(job);
        }
        Ok(finished)
    }

    /// Starts a component solve from `seed`.
    fn start_job(&mut self, seed: ElementId) -> SolveResult<Job> {
        *at_mut(&mut self.scratch.in_job, seed.index())? = true;
        self.scratch.comp.push(seed);
        self.scratch.frontier.push_back(seed);
        Ok(Job {
            phase: Phase::Collect,
            cursor: 0,
        })
    }

    /// Empties the per-job buffers, keeping their capacity.
    fn reset_scratch(&mut self) {
        self.scratch.comp.clear();
        self.scratch.order.clear();
        self.scratch.frontier.clear();
        self.scratch.decisions.clear();
    }

    /// Runs one step of the current phase.
    fn step_phase(
        &mut self,
        job: &mut Job,
        tick: Tick,
        sink: &mut EventSink,
    ) -> SolveResult<Progress> {
        match job.phase {
            Phase::Collect => self.collect_step(),
            Phase::Support => self.support_step(),
            Phase::Load => self.load_step(job),
            Phase::Decide => self.decide_step(job),
            Phase::Apply => self.apply_step(job, tick, sink),
            Phase::Release => self.release_step(job),
        }
    }

    /// Moves to the next phase once the current one has no more work. Returns true when the job is complete.
    fn finish_phase(
        &mut self,
        job: &mut Job,
        tick: Tick,
        sink: &mut EventSink,
    ) -> SolveResult<bool> {
        match job.phase {
            Phase::Collect => {
                self.scratch.comp.sort_unstable();
                if self.component_has_anchor() {
                    self.begin_support(job)?;
                } else {
                    self.scratch.decisions.clear();
                    for &id in &self.scratch.comp {
                        self.scratch.decisions.push(Decision {
                            id,
                            outcome: Outcome::Detach,
                        });
                    }
                    self.begin_apply(job, tick, sink)?;
                }
                Ok(false)
            }
            Phase::Support => {
                if self.scratch.order.len() != self.scratch.comp.len() {
                    return Err(DestructionError::Invariant(
                        "support tree does not cover the component",
                    ));
                }
                job.phase = Phase::Load;
                job.cursor = 0;
                Ok(false)
            }
            Phase::Load => {
                job.phase = Phase::Decide;
                job.cursor = 0;
                Ok(false)
            }
            Phase::Decide => {
                self.begin_apply(job, tick, sink)?;
                Ok(false)
            }
            Phase::Apply => {
                job.phase = Phase::Release;
                job.cursor = 0;
                Ok(false)
            }
            Phase::Release => Ok(true),
        }
    }
}
