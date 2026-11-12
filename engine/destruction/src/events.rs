//! Events emitted by [`StructureState::step`](crate::state::StructureState::step).
//!
//! Damage calls queue input without emitting. The step that starts the next eligible
//! cycle emits `ElementDamaged` in ascending element-id order, labelled with each
//! element's first merged hit tick. The [`EventSink`] is a reusable caller-owned
//! buffer. Call [`EventSink::clear`] before each step; the buffer keeps its capacity,
//! so steady-state steps do not allocate.

use std::ops::Range;

use engine_core::Tick;
use serde::{Deserialize, Serialize};

use crate::element::HealthBand;
use crate::ids::{ElementId, GroupId};

/// What happened. Element indices name the structure's elements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestructionEvent {
    /// Queued damage was applied at a cycle start. `band` is the resulting health band.
    ElementDamaged {
        /// Element that was hit.
        id: ElementId,
        /// Health band after the hit.
        band: HealthBand,
    },
    /// An element became overloaded and will break after `ticks_left` warning rounds.
    ElementFailing {
        /// Element that started failing.
        id: ElementId,
        /// Warning rounds before it breaks.
        ticks_left: u32,
    },
    /// An element broke.
    ElementBroken {
        /// Element that broke.
        id: ElementId,
    },
    /// A connected group of elements lost every support path and detached.
    GroupDetached {
        /// The new group.
        group: GroupId,
        /// Range of positions in [`StructureState::detached_members`](crate::state::StructureState::detached_members).
        members: Range<usize>,
    },
}

/// One event with its simulation tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Originating input tick for damage, or producing step tick for other events.
    pub tick: Tick,
    /// What happened.
    pub kind: DestructionEvent,
}

/// Reusable buffer that collects events in production order.
#[derive(Debug, Default, Clone)]
pub struct EventSink {
    events: Vec<Event>,
}

impl EventSink {
    /// Creates a sink that can hold `capacity` events before it reallocates.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity),
        }
    }

    /// Drops the collected events and keeps the capacity.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Events in the order they were produced.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Number of events collected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when no event was collected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Appends one event.
    pub(crate) fn push(&mut self, tick: Tick, kind: DestructionEvent) {
        self.events.push(Event { tick, kind });
    }
}
