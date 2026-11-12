//! Reusable generation-checked storage shared by engine-owned resource tables.
//!
//! Removal advances the generation before an index returns to the free list, so a stale id can
//! never address a replacement value. A generation that would overflow retires its slot.

/// Index and generation identifying one live slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId {
    index: u32,
    generation: u32,
}

impl SlotId {
    /// Creates an id from its slot index and generation.
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Returns the slot index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the slot generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

struct Entry<T> {
    generation: u32,
    value: Option<T>,
}

/// Reusable storage whose ids reject stale references after removal and reuse.
pub struct Slots<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Slots<T> {
    /// Creates an empty slot table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    /// Creates a slot table with storage reserved for `capacity` live values.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            free: Vec::with_capacity(capacity),
            len: 0,
        }
    }

    /// Inserts a value and returns its generation-checked id.
    pub fn insert(&mut self, value: T) -> Option<SlotId> {
        while let Some(index) = self.free.pop() {
            let Ok(slot) = usize::try_from(index) else {
                continue;
            };
            let Some(entry) = self.entries.get_mut(slot) else {
                continue;
            };
            if entry.value.is_some() {
                continue;
            }
            entry.value = Some(value);
            self.len = self.len.saturating_add(1);
            return Some(SlotId::new(index, entry.generation));
        }
        let index = u32::try_from(self.entries.len()).ok()?;
        self.entries.push(Entry {
            generation: 1,
            value: Some(value),
        });
        self.len = self.len.saturating_add(1);
        Some(SlotId::new(index, 1))
    }

    /// Returns a live value if `id` still identifies its generation.
    #[must_use]
    pub fn get(&self, id: SlotId) -> Option<&T> {
        self.entry(id)?.value.as_ref()
    }

    /// Returns a live value mutably if `id` still identifies its generation.
    pub fn get_mut(&mut self, id: SlotId) -> Option<&mut T> {
        self.entry_mut(id)?.value.as_mut()
    }

    /// Removes a live value and advances the slot generation.
    pub fn remove(&mut self, id: SlotId) -> Option<T> {
        let slot = usize::try_from(id.index).ok()?;
        let entry = self.entries.get_mut(slot)?;
        if entry.generation != id.generation {
            return None;
        }
        let value = entry.value.take()?;
        self.len = self.len.saturating_sub(1);
        if let Some(generation) = entry.generation.checked_add(1) {
            entry.generation = generation;
            self.free.push(id.index);
        }
        Some(value)
    }

    /// Number of live values.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether there are no live values.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterates over live id and value pairs in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (SlotId, &T)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let value = entry.value.as_ref()?;
                let index = u32::try_from(index).ok()?;
                Some((SlotId::new(index, entry.generation), value))
            })
    }

    /// Iterates over live id and value pairs mutably in slot order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (SlotId, &mut T)> {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(index, entry)| {
                let value = entry.value.as_mut()?;
                let index = u32::try_from(index).ok()?;
                Some((SlotId::new(index, entry.generation), value))
            })
    }

    fn entry(&self, id: SlotId) -> Option<&Entry<T>> {
        self.entries
            .get(usize::try_from(id.index).ok()?)
            .filter(|entry| entry.generation == id.generation)
    }

    fn entry_mut(&mut self, id: SlotId) -> Option<&mut Entry<T>> {
        self.entries
            .get_mut(usize::try_from(id.index).ok()?)
            .filter(|entry| entry.generation == id.generation)
    }
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::Slots;

    #[test]
    fn removed_id_is_stale_after_slot_reuse() -> Result<(), Box<dyn std::error::Error>> {
        let mut slots = Slots::new();
        let first = slots
            .insert("first")
            .ok_or("first slot was not allocated")?;
        assert_eq!(slots.remove(first), Some("first"));
        let second = slots
            .insert("second")
            .ok_or("reused slot was not allocated")?;
        assert_eq!(first.index(), second.index());
        assert_ne!(first.generation(), second.generation());
        assert_eq!(slots.get(first), None);
        assert_eq!(slots.get(second), Some(&"second"));
        Ok(())
    }

    proptest! {
        #[test]
        fn live_ids_remain_unique(ops in prop::collection::vec((any::<bool>(), any::<u8>()), 0..500)) {
            let mut slots = Slots::new();
            let mut live = Vec::new();
            for (insert, value) in ops {
                if insert || live.is_empty() {
                    if let Some(id) = slots.insert(value) {
                        live.push(id);
                    }
                } else {
                    let remove_at = usize::from(value) % live.len();
                    let id = live.remove(remove_at);
                    prop_assert!(slots.remove(id).is_some());
                    prop_assert!(slots.get(id).is_none());
                }
                let ids = live.iter().copied().collect::<HashSet<_>>();
                prop_assert_eq!(ids.len(), live.len());
                prop_assert!(live.iter().all(|id| slots.get(*id).is_some()));
            }
        }
    }
}
