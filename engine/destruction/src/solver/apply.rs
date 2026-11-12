//! Applies the recorded outcomes. Overloaded elements start their warning phase,
//! detached elements fall, and each detached group is recorded before any element
//! changes state.
//!
//! Invariant: group ids come from `next_group` in detach order, and a group is
//! recorded only when it has members.

use engine_core::Tick;

use crate::element::ElementState;
use crate::error::{DestructionError, at, at_mut};
use crate::events::{DestructionEvent, EventSink};
use crate::ids::GroupId;
use crate::state::{DetachedGroup, StructureState};

use super::SolveResult;
use super::job::{Job, Outcome, Phase, Progress};

impl StructureState {
    /// Enters the apply phase. Detached members and their group are recorded here, before any element changes state.
    pub(super) fn begin_apply(
        &mut self,
        job: &mut Job,
        tick: Tick,
        sink: &mut EventSink,
    ) -> SolveResult<()> {
        job.phase = Phase::Apply;
        job.cursor = 0;
        let start = self.detached_members.len();
        let mut mass = 0.0f32;
        for decision in self.scratch.decisions.iter() {
            if decision.outcome == Outcome::Detach {
                let def = self
                    .structure
                    .element(decision.id)
                    .ok_or(DestructionError::Invariant("detached element missing"))?;
                mass += def.mass();
                self.detached_members.push(decision.id);
            }
        }
        let end = self.detached_members.len();
        if end > start {
            let group = self.next_group_id()?;
            self.groups.push(DetachedGroup {
                id: group,
                members: start..end,
                mass,
            });
            sink.push(
                tick,
                DestructionEvent::GroupDetached {
                    group,
                    members: start..end,
                },
            );
        }
        Ok(())
    }

    /// Hands out the next group id in detach order.
    fn next_group_id(&mut self) -> SolveResult<GroupId> {
        let raw = self.next_group;
        self.next_group = raw
            .checked_add(1)
            .ok_or(DestructionError::GroupIdsExhausted)?;
        Ok(GroupId::new(raw))
    }

    /// Apply: starts the warning phase for overloaded elements and marks detached ones.
    pub(super) fn apply_step(
        &mut self,
        job: &mut Job,
        tick: Tick,
        sink: &mut EventSink,
    ) -> SolveResult<Progress> {
        let Some(&decision) = self.scratch.decisions.get(job.cursor) else {
            return Ok(Progress::PhaseDone);
        };
        job.cursor += 1;
        let index = decision.id.index();
        match decision.outcome {
            Outcome::Overload => {
                if *at(&self.states, index)? == ElementState::Intact {
                    let ticks_left = self.structure.config().min_warning_ticks().get();
                    *at_mut(&mut self.states, index)? = ElementState::Failing { ticks_left };
                    self.failing.push(decision.id);
                    sink.push(
                        tick,
                        DestructionEvent::ElementFailing {
                            id: decision.id,
                            ticks_left,
                        },
                    );
                }
            }
            Outcome::Detach => {
                *at_mut(&mut self.states, index)? = ElementState::Detached;
            }
        }
        Ok(Progress::Visited)
    }
}
