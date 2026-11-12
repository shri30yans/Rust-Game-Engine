//! Per-system entity iteration without per-tick allocation.
//!
//! A system needs the ids of every entity with a component, and then needs the world mutably for
//! each one. The ids go into a [`Roster`] buffer that the world owns: the system takes the buffer
//! out, fills it, walks it and hands it back. After the first tick the buffer keeps its capacity,
//! so nothing is allocated.
//!
//! Invariants: ids are visited in ascending [`NetId`] order (the world's order, so runs are
//! deterministic); the roster holds no simulation state and writes nothing to the state hash; the
//! buffer is always handed back, even when a visit fails.

use engine_core::StateHasher;
use engine_world::{NetId, Resource, World, WorldError};

/// Capacity reserved for the roster at install. More entities than this grow the buffer once.
pub(crate) const ROSTER_CAPACITY: usize = 256;

/// Transient id buffer owned by the world. It is empty between systems.
pub(crate) struct Roster {
    ids: Vec<NetId>,
}

impl Roster {
    /// An empty roster with [`ROSTER_CAPACITY`] reserved.
    pub(crate) fn new() -> Self {
        Self {
            ids: Vec::with_capacity(ROSTER_CAPACITY),
        }
    }
}

impl Resource for Roster {
    fn hash_into(&self, _hasher: &mut StateHasher) {
        // Transient: the roster is empty between systems, so it carries no state to hash.
    }
}

/// Calls `visit` once for every live entity that has a `C`, in ascending id order. Stops at the
/// first failed visit and returns its error.
///
/// # Errors
///
/// Returns a world error when the roster is missing, or the first error `visit` returns.
pub(crate) fn for_each_with<C, F>(world: &mut World, mut visit: F) -> Result<(), WorldError>
where
    C: Send + Sync + 'static,
    F: FnMut(&mut World, NetId) -> Result<(), WorldError>,
{
    let mut ids = std::mem::take(&mut world.resource_mut::<Roster>()?.ids);
    ids.clear();
    ids.extend(world.net_ids().filter(|id| world.get::<C>(*id).is_ok()));
    let mut result = Ok(());
    for id in ids.iter().copied() {
        result = visit(world, id);
        if result.is_err() {
            break;
        }
    }
    world.resource_mut::<Roster>()?.ids = ids;
    result
}
