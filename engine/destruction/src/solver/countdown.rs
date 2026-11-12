//! The collapse countdown. Each round removes one warning tick from every
//! `Failing` element. An element whose ticks run out breaks, and its live
//! neighbours are queued for a solve.
//!
//! Invariant: a round starts only when no solve is queued. A round that does not
//! reach the end of the failing list keeps its cursor for the next step.

use std::sync::Arc;

use engine_core::Tick;

use crate::element::ElementState;
use crate::error::{at, at_mut};
use crate::events::{DestructionEvent, EventSink};
use crate::ids::ElementId;
use crate::state::StructureState;

use super::SolveResult;

impl StructureState {
    /// Runs countdown visits until the budget is spent or the round ends.
    pub(super) fn run_countdown(
        &mut self,
        tick: Tick,
        sink: &mut EventSink,
        left: &mut usize,
        visits: &mut usize,
    ) -> SolveResult<()> {
        let mut cursor = self.countdown.unwrap_or(0);
        while *left > 0 {
            let Some(&id) = self.failing.get(cursor) else {
                break;
            };
            cursor += 1;
            *left -= 1;
            *visits += 1;
            self.countdown_one(id, tick, sink)?;
        }
        if cursor >= self.failing.len() {
            let states = &self.states;
            self.failing
                .retain(|id| matches!(states.get(id.index()), Some(ElementState::Failing { .. })));
            self.countdown = None;
        } else {
            self.countdown = Some(cursor);
        }
        Ok(())
    }

    /// Removes one warning tick from `id`, breaking it and queueing its neighbours when none remain.
    fn countdown_one(
        &mut self,
        id: ElementId,
        tick: Tick,
        sink: &mut EventSink,
    ) -> SolveResult<()> {
        let index = id.index();
        let ElementState::Failing { ticks_left } = *at(&self.states, index)? else {
            return Ok(());
        };
        let remaining = ticks_left.saturating_sub(1);
        if remaining > 0 {
            *at_mut(&mut self.states, index)? = ElementState::Failing {
                ticks_left: remaining,
            };
            return Ok(());
        }
        *at_mut(&mut self.states, index)? = ElementState::Broken;
        sink.push(tick, DestructionEvent::ElementBroken { id });
        let structure = Arc::clone(&self.structure);
        for &next in structure.neighbours(id) {
            if self.is_live(next) {
                self.enqueue(next)?;
            }
        }
        Ok(())
    }
}
