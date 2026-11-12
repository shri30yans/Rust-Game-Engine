//! Parent/child links and the only operations that edit them.
//!
//! The link components are private and written only here, so the hierarchy
//! changes solely through [`World::set_parent`], [`World::detach`] and
//! [`World::despawn`]. `set_parent` walks the ancestors of the new parent before
//! it writes anything and rejects a link that would close a loop. Each entity
//! has at most one parent, so the links form a forest and every walk up or down
//! it terminates.

use engine_core::StateHasher;
use hecs::Entity;

use crate::error::WorldError;
use crate::net_id::NetId;
use crate::world::World;

/// Link from an entity to its single parent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Parent {
    pub(crate) parent: NetId,
}

/// Ordered list of an entity's children, in the order they were attached.
#[derive(Debug, Clone, Default)]
pub(crate) struct Children {
    pub(crate) list: Vec<NetId>,
}

impl World {
    /// Makes `child` a child of `parent`, detaching it from any previous parent.
    ///
    /// # Errors
    /// Returns [`WorldError::Cycle`] when `child` is `parent` or an ancestor of
    /// it, and [`WorldError::StaleNetId`] when either id is not live. A failed
    /// call leaves the world unchanged.
    pub fn set_parent(&mut self, child: NetId, parent: NetId) -> Result<(), WorldError> {
        let child_entity = self.ids.resolve(child)?;
        let parent_entity = self.ids.resolve(parent)?;
        if child == parent {
            return Err(WorldError::Cycle { child, parent });
        }
        self.check_no_loop(child, parent)?;
        self.detach(child)?;
        self.ecs
            .insert_one(child_entity, Parent { parent })
            .map_err(|_| WorldError::Invariant("link into a missing entity"))?;
        let has_children = self.ecs.get::<&Children>(parent_entity).is_ok();
        if has_children {
            let mut children = self
                .ecs
                .get::<&mut Children>(parent_entity)
                .map_err(|_| WorldError::Invariant("children vanished during link"))?;
            children.list.push(child);
        } else {
            self.ecs
                .insert_one(parent_entity, Children { list: vec![child] })
                .map_err(|_| WorldError::Invariant("children into a missing entity"))?;
        }
        Ok(())
    }

    /// Removes the parent link of `id`, making it a root. No-op for a root.
    ///
    /// # Errors
    /// Returns [`WorldError::StaleNetId`] when `id` is not live.
    pub fn detach(&mut self, id: NetId) -> Result<(), WorldError> {
        let entity = self.ids.resolve(id)?;
        let Some(parent) = self.parent(id)? else {
            return Ok(());
        };
        let parent_entity = self.ids.resolve(parent)?;
        if let Ok(mut children) = self.ecs.get::<&mut Children>(parent_entity) {
            children.list.retain(|&child| child != id);
        }
        self.ecs
            .remove_one::<Parent>(entity)
            .map(|_| ())
            .map_err(|_| WorldError::Invariant("parent link vanished during detach"))
    }

    /// Despawns `id` and its whole subtree.
    ///
    /// Policy: descendants are not reparented or kept alive. The subtree is
    /// removed as a unit, so no entity ever outlives its parent. Returns how many
    /// entities were removed, including `id`.
    ///
    /// # Errors
    /// Returns [`WorldError::StaleNetId`] when `id` is not live. A failed call
    /// leaves the world unchanged.
    pub fn despawn(&mut self, id: NetId) -> Result<usize, WorldError> {
        self.ids.resolve(id)?;
        let mut pending = vec![id];
        let mut doomed = Vec::new();
        while let Some(current) = pending.pop() {
            let entity = self.ids.resolve(current)?;
            doomed.push(current);
            if let Ok(children) = self.ecs.get::<&Children>(entity) {
                pending.extend(children.list.iter().copied());
            }
        }
        self.detach(id)?;
        for &current in &doomed {
            let entity = self.ids.free(current)?;
            self.ecs
                .despawn(entity)
                .map_err(|_| WorldError::Invariant("live entity missing during despawn"))?;
        }
        Ok(doomed.len())
    }

    /// Parent of `id`, or `None` for a root.
    ///
    /// # Errors
    /// Returns [`WorldError::StaleNetId`] when `id` is not live.
    pub fn parent(&self, id: NetId) -> Result<Option<NetId>, WorldError> {
        let entity = self.ids.resolve(id)?;
        Ok(self.ecs.get::<&Parent>(entity).map(|p| p.parent).ok())
    }

    /// Children of `id` in attachment order.
    ///
    /// # Errors
    /// Returns [`WorldError::StaleNetId`] when `id` is not live.
    pub fn children(&self, id: NetId) -> Result<Vec<NetId>, WorldError> {
        let entity = self.ids.resolve(id)?;
        Ok(self
            .ecs
            .get::<&Children>(entity)
            .map(|c| c.list.clone())
            .unwrap_or_default())
    }

    /// Fails if `child` is `parent` or one of its ancestors. The walk is bounded
    /// by the live entity count, so a corrupted chain cannot spin forever.
    fn check_no_loop(&self, child: NetId, parent: NetId) -> Result<(), WorldError> {
        let mut cursor = Some(parent);
        for _ in 0..=self.ids.len() {
            let Some(current) = cursor else {
                return Ok(());
            };
            if current == child {
                return Err(WorldError::Cycle { child, parent });
            }
            cursor = self.parent(current)?;
        }
        Err(WorldError::Invariant(
            "parent chain is longer than the entity count",
        ))
    }
}

/// Writes the parent link of `entity` into a state hash: a presence flag, then
/// the parent's id when present.
pub(crate) fn hash_parent(ecs: &hecs::World, entity: Entity, hasher: &mut StateHasher) {
    match ecs.get::<&Parent>(entity).map(|p| p.parent).ok() {
        Some(parent) => {
            hasher.write_u8(1);
            hasher.write_u32(parent.index());
            hasher.write_u32(parent.generation());
        }
        None => hasher.write_u8(0),
    }
}
