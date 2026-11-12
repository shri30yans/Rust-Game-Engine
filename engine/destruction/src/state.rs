//! Mutable structure state: element life-cycle, health, detached groups and pending solver work.
//!
//! Damage calls only queue input. At a cycle boundary, pending hits merge per
//! element with saturating addition, apply in ascending [`ElementId`] order, and
//! start one solve/countdown cycle. Hits submitted while that cycle is active wait
//! until all of its solve and countdown work finishes, then apply when a later
//! [`StructureState::step`] starts the next cycle. Pre-step damage therefore applies
//! before the initial full-structure solve. Budget size changes completion time,
//! never the cycle in which damage takes effect.
//!
//! Invariants:
//! - `states`, `health`, `queued` and `pending_damage` each hold one entry per element.
//! - `pending` equals the number of `true` entries in `queued`.
//! - `pending_damage_ids` contains each element with queued damage exactly once.
//! - [`CycleState::Boundary`] permits a new damage batch; [`CycleState::Active`]
//!   lasts until no solve, countdown, or failing work remains.
//! - An element is in `failing` while its state is `Failing`. A solve may leave
//!   stale entries behind; the next countdown round removes them.
//! - `detached_members` holds every group's members in order, and each group's
//!   `members` range indexes it.
//! - `Broken` is terminal: no operation moves an element out of it.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

use engine_core::{StateHasher, Tick};

use crate::blast::Blast;
use crate::element::{ElementState, HealthBand};
use crate::error::{DestructionError, at, at_mut};
use crate::events::{DestructionEvent, EventSink};
use crate::ids::{ElementId, GroupId};
use crate::solver::{Job, Scratch};
use crate::structure::Structure;

/// A connected group of elements that lost every support path and fell together.
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedGroup {
    pub(crate) id: GroupId,
    pub(crate) members: Range<usize>,
    pub(crate) mass: f32,
}

impl DetachedGroup {
    /// Identifier, assigned in detach order.
    #[must_use]
    pub const fn id(&self) -> GroupId {
        self.id
    }

    /// Range of positions in [`StructureState::detached_members`] that belong to this group.
    #[must_use]
    pub fn members(&self) -> Range<usize> {
        self.members.clone()
    }

    /// Total mass of the group at the moment it detached.
    #[must_use]
    pub const fn mass(&self) -> f32 {
        self.mass
    }
}

/// Outcome of one [`StructureState::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepReport {
    /// Element visits this step spent. Never more than the budget.
    pub visits: usize,
    /// True at a cycle boundary when no queued damage or structural work remains.
    pub settled: bool,
}

/// Mass by life-cycle category. `supported` covers `Intact` and `Failing` elements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassTotals {
    /// Mass of elements that are still live.
    pub supported: f32,
    /// Mass of elements that broke.
    pub broken: f32,
    /// Mass of elements that detached and are falling.
    pub detached: f32,
}

/// One element's damage waiting for the next solve-cycle boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingDamage {
    amount: u16,
    tick: Tick,
}

/// Whether a structural cycle may accept its next queued damage batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CycleState {
    /// The previous cycle is complete; the next step may apply pending damage.
    Boundary,
    /// Solve or countdown work from the current cycle is still in progress.
    Active,
}

/// Mutable state of one structure. Damage and steps are the only ways to change it.
#[derive(Debug, Clone)]
pub struct StructureState {
    pub(crate) structure: Arc<Structure>,
    pub(crate) states: Vec<ElementState>,
    pub(crate) health: Vec<u16>,
    pub(crate) detached_members: Vec<ElementId>,
    pub(crate) groups: Vec<DetachedGroup>,
    pub(crate) next_group: u32,
    pub(crate) failing: Vec<ElementId>,
    pub(crate) queue: VecDeque<ElementId>,
    pub(crate) queued: Vec<bool>,
    pub(crate) pending: usize,
    pub(crate) scratch: Scratch,
    pub(crate) job: Option<Job>,
    pub(crate) countdown: Option<usize>,
    pub(crate) pending_damage: Vec<Option<PendingDamage>>,
    pub(crate) pending_damage_ids: Vec<ElementId>,
    pub(crate) cycle: CycleState,
}

impl StructureState {
    /// Creates the initial state: every element intact at full health, with
    /// every element queued so the first steps solve the whole structure.
    #[must_use]
    pub fn new(structure: Arc<Structure>) -> Self {
        let n = structure.len();
        let ids: Vec<ElementId> = (0..n).filter_map(ElementId::from_index).collect();
        let health: Vec<u16> = ids
            .iter()
            .map(|&id| {
                structure
                    .element(id)
                    .map_or(0, |def| def.max_health().get())
            })
            .collect();
        let queue: VecDeque<ElementId> = ids.iter().copied().collect();
        Self {
            structure,
            states: vec![ElementState::Intact; n],
            health,
            detached_members: Vec::with_capacity(n),
            groups: Vec::with_capacity(n),
            next_group: 0,
            failing: Vec::with_capacity(n),
            queue,
            queued: vec![true; n],
            pending: n,
            scratch: Scratch::new(n),
            job: None,
            countdown: None,
            pending_damage: vec![None; n],
            pending_damage_ids: Vec::with_capacity(n),
            cycle: CycleState::Boundary,
        }
    }

    /// The structure this state belongs to.
    #[must_use]
    pub fn structure(&self) -> &Structure {
        self.structure.as_ref()
    }

    /// Life-cycle state of `id`, or `None` when the id is not part of the structure.
    #[must_use]
    pub fn state(&self, id: ElementId) -> Option<ElementState> {
        self.states.get(id.index()).copied()
    }

    /// Current applied health of `id`, excluding damage queued for the next cycle.
    #[must_use]
    pub fn health(&self, id: ElementId) -> Option<u16> {
        self.health.get(id.index()).copied()
    }

    /// Detached groups, in the order they detached.
    #[must_use]
    pub fn groups(&self) -> &[DetachedGroup] {
        &self.groups
    }

    /// Members of all detached groups, in group order.
    #[must_use]
    pub fn detached_members(&self) -> &[ElementId] {
        &self.detached_members
    }

    /// True at a cycle boundary when no queued damage or structural work remains.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.cycle == CycleState::Boundary
            && self.cycle_work_complete()
            && self.pending_damage_ids.is_empty()
    }

    /// True when the active cycle has no solve or countdown work left.
    pub(crate) fn cycle_work_complete(&self) -> bool {
        self.job.is_none()
            && self.pending == 0
            && self.countdown.is_none()
            && self.failing.is_empty()
    }

    /// Mass in each life-cycle category. Summed over the categories it equals the
    /// total mass of the structure, because every element is in exactly one.
    #[must_use]
    pub fn mass_totals(&self) -> MassTotals {
        let mut totals = MassTotals {
            supported: 0.0,
            broken: 0.0,
            detached: 0.0,
        };
        for (raw, state) in self.states.iter().enumerate() {
            let mass = ElementId::from_index(raw)
                .and_then(|id| self.structure.element(id))
                .map_or(0.0, |def| def.mass());
            match state {
                ElementState::Intact | ElementState::Failing { .. } => totals.supported += mass,
                ElementState::Broken => totals.broken += mass,
                ElementState::Detached => totals.detached += mass,
            }
        }
        totals
    }

    /// Queues `amount` damage to `id` for the next eligible solve-cycle boundary.
    ///
    /// Repeated pending hits on one element merge with saturating addition. The
    /// first hit's tick labels the eventual [`DestructionEvent::ElementDamaged`].
    /// Zero damage and damage to an element already broken or detached are ignored.
    /// The supplied sink receives no event until [`Self::step`] applies the queue.
    ///
    /// # Errors
    /// Returns [`DestructionError::UnknownElement`] for an id outside the structure.
    pub fn apply_damage(
        &mut self,
        id: ElementId,
        amount: u16,
        tick: Tick,
        _sink: &mut EventSink,
    ) -> Result<(), DestructionError> {
        if self.structure.element(id).is_none() {
            return Err(DestructionError::UnknownElement(id));
        }
        let index = id.index();
        if amount == 0 || !at(&self.states, index)?.is_live() {
            return Ok(());
        }
        let slot = at_mut(&mut self.pending_damage, index)?;
        if let Some(pending) = slot {
            pending.amount = pending.amount.saturating_add(amount);
        } else {
            *slot = Some(PendingDamage { amount, tick });
            self.pending_damage_ids.push(id);
        }
        Ok(())
    }

    /// Queues a blast's damage in ascending element-id order.
    ///
    /// # Errors
    /// Propagates errors from [`StructureState::apply_damage`].
    pub fn apply_blast(
        &mut self,
        blast: &Blast,
        tick: Tick,
        sink: &mut EventSink,
    ) -> Result<(), DestructionError> {
        for raw in 0..self.states.len() {
            let id = ElementId::from_index(raw).ok_or(DestructionError::Invariant(
                "element index exceeds id space",
            ))?;
            let Some(def) = self.structure.element(id) else {
                continue;
            };
            if !self.is_live(id) {
                continue;
            }
            let amount = blast.damage_at(def.position());
            if amount > 0 {
                self.apply_damage(id, amount, tick, sink)?;
            }
        }
        Ok(())
    }

    /// Applies the queued batch in ascending element-id order without allocating.
    pub(crate) fn apply_pending_damage(
        &mut self,
        sink: &mut EventSink,
    ) -> Result<(), DestructionError> {
        self.pending_damage_ids.sort_unstable();
        for cursor in 0..self.pending_damage_ids.len() {
            let id = *at(&self.pending_damage_ids, cursor)?;
            let Some(pending) = at_mut(&mut self.pending_damage, id.index())?.take() else {
                return Err(DestructionError::Invariant("queued damage entry missing"));
            };
            if !self.is_live(id) {
                continue;
            }
            let def = self
                .structure
                .element(id)
                .ok_or(DestructionError::Invariant("damaged element missing"))?;
            let health = at_mut(&mut self.health, id.index())?;
            *health = health.saturating_sub(pending.amount);
            let band = HealthBand::of(*health, def.max_health());
            sink.push(pending.tick, DestructionEvent::ElementDamaged { id, band });
            self.enqueue(id)?;
        }
        self.pending_damage_ids.clear();
        Ok(())
    }

    /// Digest of the logical state: element states, health, detached members and
    /// groups. Queue order, solver scratch, unapplied damage and event history are
    /// excluded. Compare hashes only at settled cycle boundaries.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u64(u64::try_from(self.states.len()).unwrap_or(u64::MAX));
        for (state, health) in self.states.iter().zip(self.health.iter()) {
            match state {
                ElementState::Intact => hasher.write_u8(0),
                ElementState::Failing { ticks_left } => {
                    hasher.write_u8(1);
                    hasher.write_u32(*ticks_left);
                }
                ElementState::Broken => hasher.write_u8(2),
                ElementState::Detached => hasher.write_u8(3),
            }
            hasher.write_u32(u32::from(*health));
        }
        hasher.write_u64(u64::try_from(self.detached_members.len()).unwrap_or(u64::MAX));
        for member in &self.detached_members {
            hasher.write_u32(member.raw());
        }
        hasher.write_u64(u64::try_from(self.groups.len()).unwrap_or(u64::MAX));
        for group in &self.groups {
            hasher.write_u32(group.id.raw());
            hasher.write_u64(u64::try_from(group.members.start).unwrap_or(u64::MAX));
            hasher.write_u64(u64::try_from(group.members.end).unwrap_or(u64::MAX));
            hasher.write_f32(group.mass);
        }
        hasher.finish()
    }

    /// True when `id` exists and is `Intact` or `Failing`.
    pub(crate) fn is_live(&self, id: ElementId) -> bool {
        self.states
            .get(id.index())
            .is_some_and(|state| state.is_live())
    }
}
