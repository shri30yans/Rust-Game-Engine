//! Immutable structure topology: element definitions, undirected adjacency and anchors.
//!
//! A [`Structure`] is produced only by [`StructureBuilder::build`](crate::builder::StructureBuilder::build),
//! which proves every element is connected to an anchor. Adjacency is stored in
//! compressed sparse row form with each neighbour list sorted by id, so
//! iteration order never depends on the order edges were declared.
//!
//! Invariants: `offsets` is non-decreasing, has `len() + 1` entries, and its
//! last entry equals `adjacency.len()`.

use std::collections::VecDeque;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::element::ElementDef;
use crate::error::{DestructionError, at, at_mut};
use crate::ids::ElementId;

/// Rules that apply to every structure built with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestructionConfig {
    min_warning_ticks: NonZeroU32,
}

impl DestructionConfig {
    /// Creates a config. An overloaded element spends at least `min_warning_ticks`
    /// warning rounds in `Failing` before it breaks.
    #[must_use]
    pub const fn new(min_warning_ticks: NonZeroU32) -> Self {
        Self { min_warning_ticks }
    }

    /// Warning rounds an overloaded element spends in `Failing` before it breaks.
    #[must_use]
    pub const fn min_warning_ticks(self) -> NonZeroU32 {
        self.min_warning_ticks
    }
}

/// Immutable element graph with anchors. Built only by the builder.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    defs: Vec<ElementDef>,
    anchors: Vec<ElementId>,
    offsets: Vec<usize>,
    adjacency: Vec<ElementId>,
    config: DestructionConfig,
}

impl Structure {
    /// Assembles a structure from parts the builder has already validated.
    pub(crate) fn new(
        defs: Vec<ElementDef>,
        anchors: Vec<ElementId>,
        offsets: Vec<usize>,
        adjacency: Vec<ElementId>,
        config: DestructionConfig,
    ) -> Self {
        Self {
            defs,
            anchors,
            offsets,
            adjacency,
            config,
        }
    }

    /// Number of elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// True when the structure has no elements. A built structure never does.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Rules this structure was built with.
    #[must_use]
    pub const fn config(&self) -> DestructionConfig {
        self.config
    }

    /// Definition of `id`, or `None` when the id is not part of this structure.
    #[must_use]
    pub fn element(&self, id: ElementId) -> Option<ElementDef> {
        self.defs.get(id.index()).copied()
    }

    /// Anchored elements, in ascending id order.
    #[must_use]
    pub fn anchors(&self) -> &[ElementId] {
        &self.anchors
    }

    /// Elements directly connected to `id`, in ascending id order. Empty for unknown ids.
    #[must_use]
    pub fn neighbours(&self, id: ElementId) -> &[ElementId] {
        let index = id.index();
        let range = match (
            self.offsets.get(index),
            index.checked_add(1).and_then(|next| self.offsets.get(next)),
        ) {
            (Some(&start), Some(&end)) => start..end,
            _ => return &[],
        };
        self.adjacency.get(range).unwrap_or(&[])
    }

    /// First element with no path to an anchor, if any. Used by the builder as a final check.
    pub(crate) fn first_unanchored(&self) -> Result<Option<ElementId>, DestructionError> {
        let mut seen = vec![false; self.len()];
        let mut frontier = VecDeque::new();
        for &anchor in &self.anchors {
            *at_mut(&mut seen, anchor.index())? = true;
            frontier.push_back(anchor);
        }
        while let Some(current) = frontier.pop_front() {
            for &next in self.neighbours(current) {
                let flag = at_mut(&mut seen, next.index())?;
                if !*flag {
                    *flag = true;
                    frontier.push_back(next);
                }
            }
        }
        for (index, reached) in seen.iter().enumerate() {
            if !reached {
                let id = ElementId::from_index(index).ok_or(DestructionError::Invariant(
                    "element index exceeds id space",
                ))?;
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Unchecked read of an offset-table entry, used only by the builder while it fills adjacency.
    pub(crate) fn offset(offsets: &[usize], index: usize) -> Result<usize, DestructionError> {
        at(offsets, index).copied()
    }
}
