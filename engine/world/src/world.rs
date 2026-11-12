//! The world: owns the hecs storage, the [`NetId`] allocator, the component
//! registry and the root RNG.
//!
//! Every public method takes or returns a `NetId`. hecs entity handles stay
//! inside this crate, so a freed or foreign handle cannot be misused through the
//! API. Each spawned entity carries its hidden `NetId`, a [`Transform`] and a
//! [`GlobalTransform`].

use std::any::{TypeId, type_name};

use engine_core::{SimRng, StateHasher};
use hecs::{Component, Entity};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::WorldError;
use crate::hierarchy::hash_parent;
use crate::net_id::{NetId, NetIdAllocator};
use crate::registry::{ComponentId, ComponentRegistry, Replication};
use crate::resource::{Resource, ResourceStore};
use crate::transform::{GlobalTransform, Transform};

/// Simulation world. See the module header for the invariants it upholds.
///
/// Besides entities, a world owns its [`Resource`]s (singleton simulation state
/// such as the physics world). Resources are part of [`World::state_hash`].
pub struct World {
    pub(crate) ecs: hecs::World,
    pub(crate) ids: NetIdAllocator,
    registry: ComponentRegistry,
    rng: SimRng,
    resources: ResourceStore,
    /// Traversal buffer reused by transform propagation.
    pub(crate) propagation_stack: Vec<(Entity, glam::Mat4)>,
}

impl World {
    /// Creates an empty world seeded with `seed`. Registers the built-in
    /// `Transform` (replicated) and `GlobalTransform` (cosmetic, derived).
    ///
    /// # Errors
    /// Returns an error only if the built-in registration fails, which would be
    /// a bug in this crate.
    pub fn new(seed: u64) -> Result<Self, WorldError> {
        let mut registry = ComponentRegistry::default();
        registry.register::<Transform>(Replication::Replicated)?;
        registry.register::<GlobalTransform>(Replication::Cosmetic)?;
        Ok(Self {
            ecs: hecs::World::new(),
            ids: NetIdAllocator::default(),
            registry,
            rng: SimRng::new(seed, 0),
            resources: ResourceStore::default(),
            propagation_stack: Vec::new(),
        })
    }

    /// Stores a resource. Each type can be stored once per world.
    ///
    /// # Errors
    /// Returns [`WorldError::DuplicateResource`] if a value of `R` is already stored.
    pub fn insert_resource<R: Resource>(&mut self, value: R) -> Result<(), WorldError> {
        self.resources.insert(value)
    }

    /// Shared access to the stored resource of type `R`.
    ///
    /// # Errors
    /// Returns [`WorldError::MissingResource`] if no `R` is stored.
    pub fn resource<R: Resource>(&self) -> Result<&R, WorldError> {
        self.resources.get::<R>()
    }

    /// Exclusive access to the stored resource of type `R`.
    ///
    /// # Errors
    /// Returns [`WorldError::MissingResource`] if no `R` is stored.
    pub fn resource_mut<R: Resource>(&mut self) -> Result<&mut R, WorldError> {
        self.resources.get_mut::<R>()
    }

    /// Root RNG for this match. Systems draw from their own forked streams, not from this one.
    #[must_use]
    pub const fn rng(&self) -> &SimRng {
        &self.rng
    }

    /// Read-only view of the component registry.
    #[must_use]
    pub const fn registry(&self) -> &ComponentRegistry {
        &self.registry
    }

    /// Registers a component type with its replication class. Must happen before
    /// the type is inserted or encoded.
    ///
    /// # Errors
    /// Returns [`WorldError::DuplicateComponent`] if the type is already registered.
    pub fn register<T>(&mut self, class: Replication) -> Result<ComponentId, WorldError>
    where
        T: Component + Serialize + DeserializeOwned,
    {
        self.registry.register::<T>(class)
    }

    /// Creates an entity with an identity transform and returns its id.
    ///
    /// # Errors
    /// Returns [`WorldError::NetIdExhausted`] when the 32-bit index space is full.
    pub fn spawn(&mut self) -> Result<NetId, WorldError> {
        let id = self.ids.allocate()?;
        let entity = self
            .ecs
            .spawn((id, Transform::IDENTITY, GlobalTransform::IDENTITY));
        self.ids.bind(id, entity)?;
        Ok(id)
    }

    /// True when `id` names a live entity.
    #[must_use]
    pub fn is_alive(&self, id: NetId) -> bool {
        self.ids.resolve(id).is_ok()
    }

    /// Number of live entities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// True when no entity is alive.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.len() == 0
    }

    /// Number of NetId slots allocated so far, including ids that have been freed.
    #[must_use]
    pub fn net_id_slot_count(&self) -> usize {
        self.ids.slot_count()
    }

    /// Ids of all live entities, in ascending order.
    pub fn net_ids(&self) -> impl Iterator<Item = NetId> + '_ {
        self.ids.live_entities().map(|(id, _)| id)
    }

    /// Iterates entities that carry `A` and `B` in hecs archetype order.
    ///
    /// The hidden `NetId` is yielded with the borrowed components. It is stored
    /// on each entity but is not registered, replicated, or included in the
    /// state hash. Iteration does not allocate and never exposes a hecs entity.
    pub fn for_each2<A: Component, B: Component>(&self, mut visit: impl FnMut(NetId, (&A, &B))) {
        let mut query = self.ecs.query::<(&NetId, &A, &B)>();
        for (id, a, b) in query.iter() {
            visit(*id, (a, b));
        }
    }

    /// Iterates entities that carry `A` in hecs archetype order.
    ///
    /// The hidden `NetId` is yielded with the borrowed component. It is stored
    /// on each entity but is not registered, replicated, or included in the
    /// state hash. Iteration does not allocate and never exposes a hecs entity.
    pub fn for_each1<A: Component>(&self, mut visit: impl FnMut(NetId, &A)) {
        let mut query = self.ecs.query::<(&NetId, &A)>();
        for (id, a) in query.iter() {
            visit(*id, a);
        }
    }

    /// Shared access to a component.
    ///
    /// # Errors
    /// Returns [`WorldError::StaleNetId`] for a dead id and
    /// [`WorldError::MissingComponent`] when the entity lacks `T`.
    pub fn get<T: Component>(&self, id: NetId) -> Result<hecs::Ref<'_, T>, WorldError> {
        let entity = self.ids.resolve(id)?;
        self.ecs
            .get::<&T>(entity)
            .map_err(|_| WorldError::MissingComponent(id, type_name::<T>()))
    }

    /// Exclusive access to a component.
    ///
    /// # Errors
    /// Same as [`World::get`].
    pub fn get_mut<T: Component>(&mut self, id: NetId) -> Result<hecs::RefMut<'_, T>, WorldError> {
        let entity = self.ids.resolve(id)?;
        self.ecs
            .get::<&mut T>(entity)
            .map_err(|_| WorldError::MissingComponent(id, type_name::<T>()))
    }

    /// Adds or replaces a component. The type must be registered.
    ///
    /// # Errors
    /// Returns [`WorldError::UnknownComponent`] for an unregistered type and
    /// [`WorldError::StaleNetId`] for a dead id.
    pub fn insert<T: Component>(&mut self, id: NetId, value: T) -> Result<(), WorldError> {
        if self.registry.id_of::<T>().is_none() {
            return Err(WorldError::UnknownComponent(type_name::<T>()));
        }
        let entity = self.ids.resolve(id)?;
        self.ecs
            .insert_one(entity, value)
            .map_err(|_| WorldError::StaleNetId(id))
    }

    /// Removes a component and returns it. Built-in transform components cannot be removed.
    ///
    /// # Errors
    /// Returns [`WorldError::BuiltinComponent`] for `Transform` or `GlobalTransform`
    /// and [`WorldError::StaleNetId`] for a dead id.
    pub fn remove<T: Component>(&mut self, id: NetId) -> Result<Option<T>, WorldError> {
        let type_id = TypeId::of::<T>();
        if type_id == TypeId::of::<Transform>() || type_id == TypeId::of::<GlobalTransform>() {
            return Err(WorldError::BuiltinComponent(type_name::<T>()));
        }
        let entity = self.ids.resolve(id)?;
        Ok(self.ecs.remove_one::<T>(entity).ok())
    }

    /// Serialises one component of one entity with postcard. `None` when the entity lacks it.
    ///
    /// # Errors
    /// Returns [`WorldError::UnknownComponentId`], [`WorldError::StaleNetId`] or a codec error.
    pub fn encode_component(
        &self,
        id: NetId,
        component: ComponentId,
    ) -> Result<Option<Vec<u8>>, WorldError> {
        let mut buffer = Vec::new();
        if self.encode_component_into(id, component, &mut buffer)? {
            Ok(Some(buffer))
        } else {
            Ok(None)
        }
    }

    /// Encodes a registered component into reusable caller storage.
    ///
    /// # Errors
    /// Returns [`WorldError::UnknownComponentId`], [`WorldError::StaleNetId`] or a codec error.
    pub fn encode_component_into(
        &self,
        id: NetId,
        component: ComponentId,
        out: &mut Vec<u8>,
    ) -> Result<bool, WorldError> {
        out.clear();
        let entity = self.ids.resolve(id)?;
        let ops = self.registry.ops(component)?;
        (ops.encode)(&self.ecs, entity, out)
    }

    /// Decodes bytes produced by [`World::encode_component`] and inserts the component.
    ///
    /// # Errors
    /// Returns [`WorldError::UnknownComponentId`], [`WorldError::StaleNetId`],
    /// [`WorldError::TrailingBytes`] or a codec error.
    pub fn decode_component(
        &mut self,
        id: NetId,
        component: ComponentId,
        bytes: &[u8],
    ) -> Result<(), WorldError> {
        let entity = self.ids.resolve(id)?;
        let ops = self.registry.ops(component)?;
        (ops.decode)(&mut self.ecs, entity, bytes)
    }

    /// Removes a registered component through its type-erased registry operation.
    ///
    /// # Errors
    /// Returns [`WorldError::UnknownComponentId`], [`WorldError::StaleNetId`] or
    /// [`WorldError::BuiltinComponent`] when the component is a built-in transform.
    pub fn remove_component(
        &mut self,
        id: NetId,
        component: ComponentId,
    ) -> Result<bool, WorldError> {
        if self.registry.id_of::<Transform>() == Some(component) {
            return Err(WorldError::BuiltinComponent(type_name::<Transform>()));
        }
        if self.registry.id_of::<GlobalTransform>() == Some(component) {
            return Err(WorldError::BuiltinComponent(type_name::<GlobalTransform>()));
        }
        let entity = self.ids.resolve(id)?;
        let ops = self.registry.ops(component)?;
        (ops.remove)(&mut self.ecs, entity)
    }

    /// Hash of the whole simulation state.
    ///
    /// Covers the root RNG, the allocator (generations and free list), every live entity in id
    /// order, its parent link, and every non-cosmetic registered component in id
    /// order, using each component's postcard bytes. Cosmetic components are
    /// excluded. Then come the resources, in insertion order, each as its own
    /// [`Resource::hash_into`] bytes. Two worlds that performed the same operations
    /// hash equally, and any change to a hashed value changes the digest.
    ///
    /// # Errors
    /// Returns an error only if a component fails to encode, which indicates a
    /// broken serde implementation.
    pub fn state_hash(&self) -> Result<u64, WorldError> {
        let mut hasher = StateHasher::new();
        let mut buffer = Vec::new();
        let rng_bytes = postcard::to_stdvec(&self.rng).map_err(|source| WorldError::Codec {
            component: type_name::<SimRng>(),
            source,
        })?;
        hasher.write_bytes(&rng_bytes);
        for slot in self.ids.slots() {
            hasher.write_u32(slot.generation);
            let Some(entity) = slot.entity else {
                hasher.write_u8(0);
                continue;
            };
            hasher.write_u8(1);
            hash_parent(&self.ecs, entity, &mut hasher);
            for (component, ops) in self.registry.hashed() {
                buffer.clear();
                let present = (ops.encode)(&self.ecs, entity, &mut buffer)?;
                hasher.write_u32(component.raw());
                hasher.write_u8(u8::from(present));
                if present {
                    hasher.write_bytes(&buffer);
                }
            }
        }
        for &index in self.ids.free_list() {
            hasher.write_u8(1);
            hasher.write_u32(index);
        }
        self.resources.hash_into(&mut hasher);
        hasher.write_u8(0);
        Ok(hasher.finish())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn spawn_get_despawn_round_trip() {
        let mut world = World::new(7).unwrap();
        let id = world.spawn().unwrap();
        assert!(world.is_alive(id));
        assert_eq!(
            world.get::<Transform>(id).unwrap().translation(),
            Vec3::ZERO
        );
        assert_eq!(world.despawn(id).unwrap(), 1);
        assert!(!world.is_alive(id));
        assert!(matches!(
            world.get::<Transform>(id),
            Err(WorldError::StaleNetId(_))
        ));
        assert!(world.is_empty());
    }

    #[test]
    fn transform_cannot_be_removed() {
        let mut world = World::new(7).unwrap();
        let id = world.spawn().unwrap();
        assert!(matches!(
            world.remove::<Transform>(id),
            Err(WorldError::BuiltinComponent(_))
        ));
    }

    #[test]
    fn dense_queries_yield_live_ids_and_skip_despawned_entities() {
        let mut world = World::new(8).unwrap();
        let first = world.spawn().unwrap();
        let removed = world.spawn().unwrap();
        let last = world.spawn().unwrap();
        world.despawn(removed).unwrap();

        let mut queried = Vec::new();
        world.for_each1::<Transform>(|id, transform| {
            queried.push((id, transform.translation()));
        });

        assert_eq!(queried.len(), 2);
        assert!(queried.iter().any(|(id, _)| *id == first));
        assert!(queried.iter().any(|(id, _)| *id == last));
        assert!(!queried.iter().any(|(id, _)| *id == removed));
    }

    #[test]
    fn insert_requires_registration() {
        #[derive(Debug, PartialEq)]
        struct Unregistered;
        let mut world = World::new(7).unwrap();
        let id = world.spawn().unwrap();
        assert!(matches!(
            world.insert(id, Unregistered),
            Err(WorldError::UnknownComponent(_))
        ));
    }

    #[test]
    fn state_hash_covers_the_root_rng_seed() {
        let a = World::new(1).unwrap();
        let b = World::new(1).unwrap();
        let c = World::new(2).unwrap();
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        assert_ne!(a.state_hash().unwrap(), c.state_hash().unwrap());
    }
}
