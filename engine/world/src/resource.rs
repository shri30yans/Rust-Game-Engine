//! Resources: simulation state that is not an entity (the physics world, a match
//! clock, a map). Each [`World`](crate::World) owns its own store, so systems reach
//! state through the world and no global exists.
//!
//! Invariants: one value per type per world; entries keep insertion order, so the
//! state hash is deterministic; every resource states how it enters the state hash.
//! Insertion allocates once. Reads and writes never allocate.

use std::any::{Any, type_name};

use engine_core::StateHasher;

use crate::error::WorldError;

/// A value stored once per world. It must say how it enters the state hash.
pub trait Resource: Any + Send + Sync {
    /// Writes everything that is part of simulation state. Two equal states write equal bytes.
    fn hash_into(&self, hasher: &mut StateHasher);
}

/// Private storage for the resources of one world.
#[derive(Default)]
pub(crate) struct ResourceStore {
    entries: Vec<Box<dyn Resource>>,
}

impl ResourceStore {
    /// Stores `value`, failing if a resource of the same type is already present.
    pub(crate) fn insert<R: Resource>(&mut self, value: R) -> Result<(), WorldError> {
        if self.find::<R>().is_some() {
            return Err(WorldError::DuplicateResource(type_name::<R>()));
        }
        self.entries.push(Box::new(value));
        Ok(())
    }

    /// Shared access to the stored `R`.
    pub(crate) fn get<R: Resource>(&self) -> Result<&R, WorldError> {
        self.find::<R>()
            .ok_or(WorldError::MissingResource(type_name::<R>()))
    }

    /// Exclusive access to the stored `R`.
    pub(crate) fn get_mut<R: Resource>(&mut self) -> Result<&mut R, WorldError> {
        self.entries
            .iter_mut()
            .find_map(|entry| {
                let any: &mut dyn Any = &mut **entry;
                any.downcast_mut::<R>()
            })
            .ok_or(WorldError::MissingResource(type_name::<R>()))
    }

    /// Writes every resource in insertion order. Each entry is tagged, so the
    /// boundary between two resources is part of the hashed bytes.
    pub(crate) fn hash_into(&self, hasher: &mut StateHasher) {
        for entry in &self.entries {
            hasher.write_u8(1);
            entry.hash_into(hasher);
        }
    }

    fn find<R: Resource>(&self) -> Option<&R> {
        self.entries.iter().find_map(|entry| {
            let any: &dyn Any = &**entry;
            any.downcast_ref::<R>()
        })
    }
}
