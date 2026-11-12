//! One client's acknowledged shadow of each tracked entity: the newest acknowledged version
//! of every component, the accumulator that carries unsent entities forward, and the
//! dirty and acknowledgement checks built on them.

use engine_core::Tick;
use engine_world::NetId;

use crate::snapshot::{ComponentState, EntityState};

use super::policy::EntityReplication;

/// What one client has been sent and has acknowledged for one entity.
pub(super) struct Tracked {
    pub(super) id: NetId,
    pub(super) accumulator: f32,
    pub(super) state: EntityState,
    pub(super) policy: EntityReplication,
    pub(super) last_sent: Option<Tick>,
    pub(super) sleeping: bool,
    /// The write in which this entity last appeared in the server's states.
    pub(super) seen: u64,
    /// The newest acknowledged version of each component this client has received.
    pub(super) shadow: Vec<Shadow>,
}

#[derive(Clone, Copy)]
pub(super) struct Shadow {
    component: u32,
    acked: Option<Tick>,
}

impl Tracked {
    pub(super) fn acked(&self, component: u32) -> Option<Tick> {
        acked_in(&self.shadow, component)
    }

    pub(super) fn is_dirty(&self, component: &ComponentState) -> bool {
        let shadow = self.acked(component.component);
        shadow.is_none_or(|last| component.version.is_after(last))
            && !(component.initial_only && !component.removed && shadow.is_some())
    }

    pub(super) fn acknowledge(&mut self, component: u32, version: Tick) {
        if let Some(entry) = self
            .shadow
            .iter_mut()
            .find(|entry| entry.component == component)
        {
            if entry.acked.is_none_or(|last| version.is_after(last)) {
                entry.acked = Some(version);
            }
        } else {
            self.shadow.push(Shadow {
                component,
                acked: Some(version),
            });
        }
    }

    pub(super) fn forget_shadow(&mut self) {
        for entry in &mut self.shadow {
            entry.acked = None;
        }
    }
}

fn acked_in(shadow: &[Shadow], component: u32) -> Option<Tick> {
    shadow
        .iter()
        .find(|entry| entry.component == component)
        .and_then(|entry| entry.acked)
}

/// True when the client has acknowledged every component of `current` as the writer would
/// version it this tick, so a dormant entity can sleep.
pub(super) fn state_is_acknowledged(tracked: &Tracked, current: &EntityState, tick: Tick) -> bool {
    tracked.state.components.iter().all(|old| {
        let same = current.components.iter().any(|new| {
            new.component == old.component && new.bytes == old.bytes && new.removed == old.removed
        });
        let version = if same { old.version } else { tick };
        tracked
            .acked(old.component)
            .is_some_and(|acked| acked == version || acked.is_after(version))
    }) && current.components.iter().all(|new| {
        tracked
            .state
            .components
            .iter()
            .any(|old| old.component == new.component)
            || tracked
                .acked(new.component)
                .is_some_and(|acked| acked == tick || acked.is_after(tick))
    })
}
