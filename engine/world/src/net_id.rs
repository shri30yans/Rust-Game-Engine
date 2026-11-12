//! Stable network identifiers and their allocator.
//!
//! A [`NetId`] is an `(index, generation)` pair. The allocator recycles indices,
//! but freeing a slot bumps its generation first, so the pair handed out for a
//! slot never repeats. A slot whose generation counter runs out is retired rather
//! than recycled. Any id that is freed, never bound, or from another world fails
//! every lookup. Entity handles from hecs never leave this crate.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::error::WorldError;

/// Stable, never-reused identifier for a world entity. Only the world creates these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NetId {
    index: u32,
    generation: u32,
}

impl NetId {
    /// Creates an id from its parts. Crate-private: ids come from the allocator.
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Slot index. Indices are recycled, so compare whole ids, not indices.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when this id was handed out.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// One allocator slot. `entity` is `Some` only while the id is live.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) generation: u32,
    pub(crate) entity: Option<Entity>,
}

/// Hands out [`NetId`]s and maps them to hecs entities.
#[derive(Debug, Default)]
pub(crate) struct NetIdAllocator {
    slots: Vec<Slot>,
    /// Indices whose generation was already bumped, in the order they were freed.
    free: Vec<u32>,
    live: usize,
}

impl NetIdAllocator {
    /// Reserves an id. The caller must [`bind`](Self::bind) it to an entity.
    pub(crate) fn allocate(&mut self) -> Result<NetId, WorldError> {
        if let Some(index) = self.free.pop() {
            let generation = self
                .slot(index)
                .map(|slot| slot.generation)
                .ok_or(WorldError::Invariant("free list names an unknown slot"))?;
            return Ok(NetId::new(index, generation));
        }
        let index = u32::try_from(self.slots.len()).map_err(|_| WorldError::NetIdExhausted)?;
        self.slots.push(Slot {
            generation: 0,
            entity: None,
        });
        Ok(NetId::new(index, 0))
    }

    /// Attaches a reserved id to its entity.
    pub(crate) fn bind(&mut self, id: NetId, entity: Entity) -> Result<(), WorldError> {
        let slot = usize::try_from(id.index)
            .ok()
            .and_then(|i| self.slots.get_mut(i))
            .ok_or(WorldError::Invariant("bind to an unknown slot"))?;
        if slot.generation != id.generation || slot.entity.is_some() {
            return Err(WorldError::Invariant("bind to a slot that is not reserved"));
        }
        slot.entity = Some(entity);
        self.live += 1;
        Ok(())
    }

    /// Returns the entity behind a live id.
    pub(crate) fn resolve(&self, id: NetId) -> Result<Entity, WorldError> {
        self.slot(id.index)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.entity)
            .ok_or(WorldError::StaleNetId(id))
    }

    /// Retires a live id: clears its entity, bumps the generation and queues the
    /// index for reuse. Returns the entity that was bound.
    pub(crate) fn free(&mut self, id: NetId) -> Result<Entity, WorldError> {
        let entity = self.resolve(id)?;
        let slot = usize::try_from(id.index)
            .ok()
            .and_then(|i| self.slots.get_mut(i))
            .ok_or(WorldError::Invariant("free of an unknown slot"))?;
        slot.entity = None;
        self.live -= 1;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(id.index);
        }
        // Otherwise the generation is exhausted: the slot stays out of the free
        // list forever, so no id it produced can ever be handed out again.
        Ok(entity)
    }

    /// Number of live ids.
    pub(crate) const fn len(&self) -> usize {
        self.live
    }

    /// Number of slots ever allocated, including free and retired slots.
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Every slot in index order, live or not. Used for state hashing.
    pub(crate) fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// Indices waiting for reuse, in free order. Used for state hashing.
    pub(crate) fn free_list(&self) -> &[u32] {
        &self.free
    }

    /// Live ids with their entities, in index order. Iteration allocates nothing.
    pub(crate) fn live_entities(&self) -> impl Iterator<Item = (NetId, Entity)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, slot)| {
            let entity = slot.entity?;
            let index = u32::try_from(i).ok()?;
            Some((NetId::new(index, slot.generation), entity))
        })
    }

    fn slot(&self, index: u32) -> Option<&Slot> {
        usize::try_from(index).ok().and_then(|i| self.slots.get(i))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn entity(n: u32) -> Entity {
        let mut world = hecs::World::new();
        let mut last = None;
        for _ in 0..=n {
            last = Some(world.spawn(()));
        }
        last.unwrap()
    }

    #[test]
    fn freed_index_comes_back_with_new_generation() {
        let mut ids = NetIdAllocator::default();
        let first = ids.allocate().unwrap();
        ids.bind(first, entity(0)).unwrap();
        ids.free(first).unwrap();
        let second = ids.allocate().unwrap();
        assert_eq!(first.index(), second.index());
        assert_ne!(first, second);
        assert!(ids.resolve(first).is_err());
    }

    #[test]
    fn unbound_reservation_does_not_resolve() {
        let mut ids = NetIdAllocator::default();
        let id = ids.allocate().unwrap();
        assert!(matches!(ids.resolve(id), Err(WorldError::StaleNetId(_))));
    }

    #[test]
    fn exhausted_generation_retires_slot() {
        let mut ids = NetIdAllocator::default();
        let id = ids.allocate().unwrap();
        ids.bind(id, entity(0)).unwrap();
        if let Some(slot) = ids.slots.get_mut(0) {
            slot.generation = u32::MAX;
        }
        let max_id = NetId::new(0, u32::MAX);
        ids.free(max_id).unwrap();
        assert!(ids.free_list().is_empty());
        let next = ids.allocate().unwrap();
        assert_ne!(next.index(), 0);
    }
}
