//! World capture and application through registered component codecs.

use engine_core::Tick;
use engine_world::{NetId, Transform, World};

use crate::error::NetError;
use crate::snapshot::{
    ComponentState, EntityState, NetCodecRegistry, NetOwner, TransformBounds, decode_transform,
    encode_transform,
};

/// Captures serializable components according to their world replication class.
/// Owner-only filtering is applied through the registered [`NetOwner`] component.
///
/// # Errors
/// Propagates component serialization errors from [`World`].
pub fn capture_world(
    world: &World,
    recipient: Option<crate::protocol::ClientIndex>,
    owner_component: Option<engine_world::ComponentId>,
    transform_bounds: TransformBounds,
    codecs: &NetCodecRegistry,
) -> Result<Vec<EntityState>, NetError> {
    let mut states = Vec::with_capacity(world.len());
    let mut scratch = Vec::new();
    capture_world_into(
        world,
        recipient,
        owner_component,
        transform_bounds,
        codecs,
        &mut states,
        &mut scratch,
    )?;
    Ok(states)
}

/// Captures into caller-owned entity and byte buffers for per-tick use.
///
/// # Errors
/// Propagates component serialization errors from [`World`].
pub fn capture_world_into(
    world: &World,
    recipient: Option<crate::protocol::ClientIndex>,
    owner_component: Option<engine_world::ComponentId>,
    transform_bounds: TransformBounds,
    codecs: &NetCodecRegistry,
    states: &mut Vec<EntityState>,
    scratch: &mut Vec<u8>,
) -> Result<(), NetError> {
    use engine_world::Replication;

    for state in states.iter_mut() {
        for component in &mut state.components {
            component.removed = true;
        }
    }
    let mut state_index = 0;
    for id in world.net_ids() {
        if state_index == states.len() {
            states.push(EntityState::with_capacity(id, world.registry().len()));
        }
        let state = states
            .get_mut(state_index)
            .ok_or(NetError::InvalidSnapshot("capture state capacity mismatch"))?;
        state.id = id;
        let owner_matches = owner_matches(world, id, owner_component, recipient, scratch)?;
        for (component, class) in world.registry().components() {
            let should_send = class == Replication::Replicated
                || class == Replication::InitialOnly
                || (class == Replication::OwnerOnly && owner_matches);
            if !should_send {
                continue;
            }
            let component_index = state
                .components
                .iter()
                .position(|current| current.component == component.raw());
            let index = if let Some(index) = component_index {
                index
            } else {
                state.components.push(ComponentState {
                    component: component.raw(),
                    version: Tick::new(1),
                    initial_only: class == Replication::InitialOnly,
                    removed: true,
                    bytes: Vec::new(),
                });
                state.components.len().saturating_sub(1)
            };
            let value = state
                .components
                .get_mut(index)
                .ok_or(NetError::InvalidSnapshot(
                    "capture component index mismatch",
                ))?;
            value.initial_only = class == Replication::InitialOnly;
            let encoded = if world.registry().id_of::<Transform>() == Some(component) {
                let transform = world
                    .get::<Transform>(id)
                    .map_err(|error| NetError::World(error.to_string()))?;
                encode_transform(&transform, transform_bounds, &mut value.bytes)?;
                true
            } else {
                codecs.encode_into(world, id, component, &mut value.bytes)?
            };
            value.removed = !encoded;
        }
        state_index += 1;
    }
    states.truncate(state_index);
    Ok(())
}

fn owner_matches(
    world: &World,
    id: NetId,
    owner_component: Option<engine_world::ComponentId>,
    recipient: Option<crate::protocol::ClientIndex>,
    scratch: &mut Vec<u8>,
) -> Result<bool, NetError> {
    let Some(component) = owner_component else {
        return Ok(false);
    };
    if !world
        .encode_component_into(id, component, scratch)
        .map_err(|error| NetError::World(error.to_string()))?
    {
        return Ok(false);
    }
    let owner: NetOwner = postcard::from_bytes(scratch).map_err(NetError::Decode)?;
    Ok(Some(owner.client) == recipient)
}

/// Applies complete component values to an already mirrored client entity.
///
/// # Errors
/// Returns an error when the client entity or a component codec rejects the state.
pub fn apply_entity(
    world: &mut World,
    local_id: NetId,
    state: &EntityState,
    codecs: &NetCodecRegistry,
    transform_bounds: TransformBounds,
) -> Result<(), NetError> {
    let transform_id = world.registry().id_of::<Transform>();
    for component_state in &state.components {
        let component = world
            .registry()
            .components()
            .find(|(id, _)| id.raw() == component_state.component)
            .map(|(id, _)| id)
            .ok_or(NetError::InvalidSnapshot("unknown component id"))?;
        if Some(component) == transform_id {
            if component_state.removed {
                return Err(NetError::InvalidSnapshot(
                    "built-in transform cannot be removed",
                ));
            }
            let transform = decode_transform(&component_state.bytes, transform_bounds)?;
            world
                .insert(local_id, transform)
                .map_err(|error| NetError::World(error.to_string()))?;
        } else {
            if component_state.removed {
                world
                    .remove_component(local_id, component)
                    .map_err(|error| NetError::World(error.to_string()))?;
            } else {
                codecs.decode(world, local_id, component, &component_state.bytes)?;
            }
        }
    }
    Ok(())
}
