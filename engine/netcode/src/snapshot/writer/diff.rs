//! State diffing: copies a server state into a tracked copy, recording the version of each
//! changed component and marking components the server dropped as removed.

use engine_core::Tick;

use crate::snapshot::EntityState;

pub(super) fn copy_components(target: &mut EntityState, source: &EntityState, tick: Tick) {
    for source_component in &source.components {
        let index = target
            .components
            .iter()
            .position(|old| old.component == source_component.component);
        if let Some(index) = index {
            if let Some(component) = target.components.get_mut(index) {
                let changed = component.bytes != source_component.bytes
                    || component.removed != source_component.removed;
                component.version = if changed { tick } else { component.version };
                component.initial_only = source_component.initial_only;
                component.removed = source_component.removed;
                component.bytes.clone_from(&source_component.bytes);
            }
        } else {
            let mut component = source_component.clone();
            component.version = tick;
            target.components.push(component);
        }
    }
    for old in &mut target.components {
        if !source
            .components
            .iter()
            .any(|component| component.component == old.component)
        {
            if !old.removed {
                old.version = tick;
            }
            old.removed = true;
        }
    }
}

pub(super) fn update_state(target: &mut EntityState, source: &EntityState, tick: Tick) -> bool {
    let unchanged = target.components.len() == source.components.len()
        && target.components.iter().all(|old| {
            source.components.iter().any(|new| {
                new.component == old.component
                    && new.bytes == old.bytes
                    && new.removed == old.removed
            })
        });
    copy_components(target, source, tick);
    unchanged
}
