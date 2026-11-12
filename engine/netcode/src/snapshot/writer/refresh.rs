//! Per-write state sync: brings every tracked entity up to date with the server's states,
//! starts new ones, closes irrelevant ones and queues despawns for entities that vanished.

use engine_core::Tick;
use engine_world::NetId;

use crate::snapshot::{EntityState, UNCHANGED_PRIORITY_FACTOR};

use super::diff::{copy_components, update_state};
use super::policy::EntityReplication;
use super::state::{SnapshotWriter, slot_of};
use super::tracked::{Tracked, state_is_acknowledged};

impl SnapshotWriter {
    /// Brings every tracked entity up to date with `states`: new entities start tracked,
    /// irrelevant ones are closed and forget their shadow, missing ones are despawned.
    pub(super) fn refresh(
        &mut self,
        tick: Tick,
        states: &[EntityState],
        policy_for: &mut impl FnMut(NetId) -> EntityReplication,
    ) {
        self.write_count = self.write_count.wrapping_add(1);
        let write = self.write_count;
        for state in states {
            let policy = policy_for(state.id);
            let index = slot_of(state.id);
            if index >= self.slots.len() {
                if !policy.relevant {
                    continue;
                }
                self.slots.resize_with(index.saturating_add(1), || None);
            }
            let Some(slot) = self.slots.get_mut(index) else {
                continue;
            };
            if slot.as_ref().is_some_and(|tracked| tracked.id != state.id) {
                // The index was recycled for a new entity: the old one is gone.
                if let Some(old) = slot.take()
                    && !self.pending_despawns.contains(&old.id)
                {
                    self.pending_despawns.push(old.id);
                }
            }
            if !policy.relevant {
                if let Some(tracked) = slot.as_mut() {
                    if tracked.policy.relevant && !self.pending_despawns.contains(&state.id) {
                        self.pending_despawns.push(state.id);
                    }
                    tracked.policy = policy;
                    tracked.seen = write;
                    tracked.forget_shadow();
                }
                continue;
            }
            if !self.pending_despawns.is_empty() {
                self.pending_despawns.retain(|pending| *pending != state.id);
            }
            let weight = policy.priority.max(0.0);
            if let Some(tracked) = slot.as_mut() {
                let all_acknowledged = state_is_acknowledged(tracked, state, tick);
                let unchanged = update_state(&mut tracked.state, state, tick);
                let elapsed = tracked
                    .last_sent
                    .map_or(1, |last| tick.distance(last).max(1));
                let dirty = tracked
                    .state
                    .components
                    .iter()
                    .any(|component| tracked.is_dirty(component));
                if dirty && (!policy.dormant || !unchanged) {
                    tracked.accumulator += weight
                        * ticks_as_f32(elapsed)
                        * if unchanged {
                            UNCHANGED_PRIORITY_FACTOR
                        } else {
                            1.0
                        };
                }
                tracked.policy = policy;
                tracked.seen = write;
                tracked.sleeping = policy.dormant && unchanged && all_acknowledged;
            } else {
                let mut initial = EntityState::with_capacity(state.id, state.components.len());
                copy_components(&mut initial, state, tick);
                initial.reserve_component_bytes(self.max_component_bytes);
                *slot = Some(Tracked {
                    id: state.id,
                    accumulator: weight,
                    state: initial,
                    policy,
                    last_sent: None,
                    sleeping: false,
                    seen: write,
                    shadow: Vec::with_capacity(state.components.len()),
                });
            }
        }
        self.order.clear();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let Some(tracked) = slot else {
                continue;
            };
            if tracked.seen != write {
                if !self.pending_despawns.contains(&tracked.id) {
                    self.pending_despawns.push(tracked.id);
                }
                *slot = None;
                continue;
            }
            if let Ok(index) = u32::try_from(index) {
                self.order.push(index);
            }
        }
    }
}

fn ticks_as_f32(ticks: u32) -> f32 {
    let high = u16::try_from(ticks >> 16).unwrap_or(u16::MAX);
    let low = u16::try_from(ticks & u32::from(u16::MAX)).unwrap_or(u16::MAX);
    f32::from(high) * 65_536.0 + f32::from(low)
}
