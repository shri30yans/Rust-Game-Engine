//! Component registry: the list of component types a world may store, each with
//! its replication class and a postcard codec. Ids are assigned in registration
//! order and never change for the life of the registry.
//!
//! Codecs are plain function pointers generated per type, so the registry holds
//! no boxed closures and encoding into a reused buffer does not allocate.

use std::any::{TypeId, type_name};

use hecs::{Component, Entity};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::WorldError;

/// Who may see a component's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Replication {
    /// Sent to every client and included in the state hash.
    Replicated,
    /// Sent to each relevant client once, when the entity first replicates.
    InitialOnly,
    /// Sent only to the owning client and included in the state hash.
    OwnerOnly,
    /// Kept on the server and included in the state hash.
    ServerOnly,
    /// Client-local presentation or derived data. Never sent, and excluded from
    /// the state hash because peers may legitimately disagree about it.
    Cosmetic,
}

/// Stable index of a registered component type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentId(u32);

impl ComponentId {
    /// Raw index value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Encodes the component on an entity into `out`, appending. Returns `false`
/// and writes nothing when the entity lacks the component.
pub(crate) type EncodeFn = fn(&hecs::World, Entity, &mut Vec<u8>) -> Result<bool, WorldError>;
/// Decodes `bytes` and inserts the component on an entity.
pub(crate) type DecodeFn = fn(&mut hecs::World, Entity, &[u8]) -> Result<(), WorldError>;
/// Removes the registered component from an entity; returns whether it existed.
pub(crate) type RemoveFn = fn(&mut hecs::World, Entity) -> Result<bool, WorldError>;

/// Type-erased codec for one registered component type.
#[derive(Clone, Copy)]
pub(crate) struct ComponentOps {
    pub(crate) encode: EncodeFn,
    pub(crate) decode: DecodeFn,
    pub(crate) remove: RemoveFn,
}

struct Entry {
    id: ComponentId,
    name: &'static str,
    class: Replication,
    type_id: TypeId,
    ops: ComponentOps,
}

/// Every component type a world knows, in registration order.
#[derive(Default)]
pub struct ComponentRegistry {
    entries: Vec<Entry>,
}

impl ComponentRegistry {
    /// Registers `T` under the given replication class.
    ///
    /// # Errors
    /// Returns [`WorldError::DuplicateComponent`] if `T` is already registered.
    pub(crate) fn register<T>(&mut self, class: Replication) -> Result<ComponentId, WorldError>
    where
        T: Component + Serialize + DeserializeOwned,
    {
        let type_id = TypeId::of::<T>();
        if self.entries.iter().any(|entry| entry.type_id == type_id) {
            return Err(WorldError::DuplicateComponent(type_name::<T>()));
        }
        let raw = u32::try_from(self.entries.len()).map_err(|_| WorldError::TooManyComponents)?;
        let id = ComponentId(raw);
        self.entries.push(Entry {
            id,
            name: type_name::<T>(),
            class,
            type_id,
            ops: ComponentOps {
                encode: encode_op::<T>,
                decode: decode_op::<T>,
                remove: remove_op::<T>,
            },
        });
        Ok(id)
    }

    /// Id of a registered type, or `None` when `T` is not registered.
    #[must_use]
    pub fn id_of<T: Component>(&self) -> Option<ComponentId> {
        let type_id = TypeId::of::<T>();
        self.entries
            .iter()
            .find(|entry| entry.type_id == type_id)
            .map(|entry| entry.id)
    }

    /// Replication class of a registered component.
    #[must_use]
    pub fn class(&self, id: ComponentId) -> Option<Replication> {
        self.entry(id).map(|entry| entry.class)
    }

    /// Type name of a registered component.
    #[must_use]
    pub fn name(&self, id: ComponentId) -> Option<&'static str> {
        self.entry(id).map(|entry| entry.name)
    }

    /// Number of registered component types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no component type is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Components and replication classes in stable registration order.
    pub fn components(&self) -> impl Iterator<Item = (ComponentId, Replication)> + '_ {
        self.entries.iter().map(|entry| (entry.id, entry.class))
    }

    pub(crate) fn ops(&self, id: ComponentId) -> Result<ComponentOps, WorldError> {
        self.entry(id)
            .map(|entry| entry.ops)
            .ok_or(WorldError::UnknownComponentId(id))
    }

    /// Components that take part in the state hash, in id order.
    pub(crate) fn hashed(&self) -> impl Iterator<Item = (ComponentId, ComponentOps)> + '_ {
        self.entries
            .iter()
            .filter(|entry| entry.class != Replication::Cosmetic)
            .map(|entry| (entry.id, entry.ops))
    }

    fn entry(&self, id: ComponentId) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

fn encode_op<T>(ecs: &hecs::World, entity: Entity, out: &mut Vec<u8>) -> Result<bool, WorldError>
where
    T: Component + Serialize,
{
    let value = match ecs.get::<&T>(entity) {
        Ok(value) => value,
        Err(hecs::ComponentError::MissingComponent(_)) => return Ok(false),
        Err(hecs::ComponentError::NoSuchEntity) => {
            return Err(WorldError::Invariant("encode of a missing entity"));
        }
    };
    // Lend the buffer to postcard and take it back, so no new allocation is made
    // once the buffer has grown.
    let buffer = std::mem::take(out);
    let encoded = postcard::to_extend(&*value, buffer).map_err(|source| WorldError::Codec {
        component: type_name::<T>(),
        source,
    })?;
    *out = encoded;
    Ok(true)
}

fn decode_op<T>(ecs: &mut hecs::World, entity: Entity, bytes: &[u8]) -> Result<(), WorldError>
where
    T: Component + DeserializeOwned,
{
    let (value, rest) =
        postcard::take_from_bytes::<T>(bytes).map_err(|source| WorldError::Codec {
            component: type_name::<T>(),
            source,
        })?;
    if !rest.is_empty() {
        return Err(WorldError::TrailingBytes(type_name::<T>()));
    }
    ecs.insert_one(entity, value)
        .map_err(|_| WorldError::Invariant("decode into a missing entity"))
}

fn remove_op<T>(ecs: &mut hecs::World, entity: Entity) -> Result<bool, WorldError>
where
    T: Component,
{
    match ecs.remove_one::<T>(entity) {
        Ok(_) => Ok(true),
        Err(hecs::ComponentError::MissingComponent(_)) => Ok(false),
        Err(hecs::ComponentError::NoSuchEntity) => {
            Err(WorldError::Invariant("remove from a missing entity"))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Probe(u32);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Other(u8);

    #[test]
    fn ids_follow_registration_order() {
        let mut registry = ComponentRegistry::default();
        let probe = registry.register::<Probe>(Replication::Replicated).unwrap();
        let other = registry.register::<Other>(Replication::ServerOnly).unwrap();
        assert_eq!(probe.raw(), 0);
        assert_eq!(other.raw(), 1);
        assert_eq!(registry.id_of::<Other>(), Some(other));
        assert_eq!(registry.class(other), Some(Replication::ServerOnly));
    }

    #[test]
    fn duplicate_registration_is_an_error() {
        let mut registry = ComponentRegistry::default();
        registry.register::<Probe>(Replication::Replicated).unwrap();
        assert!(matches!(
            registry.register::<Probe>(Replication::OwnerOnly),
            Err(WorldError::DuplicateComponent(_))
        ));
    }

    #[test]
    fn unknown_type_and_id_are_errors() {
        let registry = ComponentRegistry::default();
        assert_eq!(registry.id_of::<Probe>(), None);
        assert!(matches!(
            registry.ops(ComponentId(7)),
            Err(WorldError::UnknownComponentId(_))
        ));
    }
}
