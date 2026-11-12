//! Extraction: copies every entity's transform from the world into a pre-sized
//! [`RenderFrame`], together with the transform the same entity had at the
//! previous extract, so drawing can interpolate between the last two ticks.
//!
//! The extractor copies values and never keeps a borrow of the world. Once the
//! extractor and the frames are built, an extract call does not allocate.
//! Invariants: transforms and visuals are dense; id lookups use generation-checked
//! slot tables, never a search. `previous` is the value the same id had at the
//! previous successful call; a call that fails
//! leaves the extractor untouched. Every live entity carries a `Transform`
//! (`World::spawn` attaches one and `World::remove` refuses to remove it), so a
//! missing `Transform` is a broken world invariant and is reported as an error.

use engine_core::Tick;
use engine_scene::Visual;
use engine_world::{NetId, Transform, World};

use crate::blend::blend;
use crate::error::AppError;

/// Capacity required for a loaded world plus the game's reserved simulation entities.
pub(crate) fn entity_capacity(world: &World, dynamic_budget: usize) -> Result<usize, AppError> {
    let entities = world.len();
    entities
        .checked_add(dynamic_budget)
        .ok_or(AppError::EntityCapacityOverflow {
            entities,
            dynamic_budget,
        })
}

/// One entity's transform at the last two ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractedTransform {
    /// The entity this transform belongs to.
    pub id: NetId,
    /// The transform at the previous extract. Equal to `current` for a new entity.
    pub previous: Transform,
    /// The transform at this extract.
    pub current: Transform,
}

/// One entity's drawing description, copied from its `Visual` component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractedVisual {
    /// The entity this visual belongs to. Always also present in the frame's transforms.
    pub id: NetId,
    /// What the entity looks like.
    pub visual: Visual,
}

/// What drawing needs for one simulated tick. Pre-sized; never grows.
#[derive(Debug)]
pub struct RenderFrame {
    tick: Tick,
    transforms: Vec<ExtractedTransform>,
    visuals: Vec<ExtractedVisual>,
    transform_slots: Vec<Option<(u32, usize)>>,
    visual_slots: Vec<Option<(u32, usize)>>,
    capacity: usize,
}

impl RenderFrame {
    /// Creates an empty frame at tick zero that can hold `capacity` transforms and as many
    /// visuals.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let transforms = Vec::with_capacity(capacity);
        let visuals = Vec::with_capacity(capacity);
        let capacity = transforms.capacity();
        Self {
            tick: Tick::ZERO,
            transforms,
            visuals,
            transform_slots: Vec::with_capacity(capacity),
            visual_slots: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// The tick this frame was extracted at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The extracted transforms in dense world-query order.
    #[must_use]
    pub fn transforms(&self) -> &[ExtractedTransform] {
        &self.transforms
    }

    /// The extracted visuals in dense world-query order. Only entities that carry a `Visual`
    /// are listed.
    #[must_use]
    pub fn visuals(&self) -> &[ExtractedVisual] {
        &self.visuals
    }

    /// The visual record of `id`, or `None` when the id has no visual in this frame.
    #[must_use]
    pub fn visual_entry(&self, id: NetId) -> Option<&ExtractedVisual> {
        let slot = usize::try_from(id.index()).ok()?;
        let (generation, dense_index) = self.visual_slots.get(slot)?.as_ref().copied()?;
        (generation == id.generation())
            .then(|| self.visuals.get(dense_index))
            .flatten()
    }

    /// The fixed number of transforms this frame can hold without growing.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// The transform record of `id`, or `None` when the id is not in this frame. The slot
    /// generation is checked, so a recycled index cannot return an old entity's transform.
    #[must_use]
    pub fn transform_entry(&self, id: NetId) -> Option<&ExtractedTransform> {
        let slot = usize::try_from(id.index()).ok()?;
        let (generation, dense_index) = self.transform_slots.get(slot)?.as_ref().copied()?;
        (generation == id.generation())
            .then(|| self.transforms.get(dense_index))
            .flatten()
    }

    /// The transform of `id` blended between the previous and the current tick at `alpha`.
    ///
    /// Returns `None` when the id is not in this frame.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::BlendAlpha`] when `alpha` is outside `[0, 1]`.
    pub fn blended(&self, id: NetId, alpha: f32) -> Result<Option<Transform>, AppError> {
        self.transform_entry(id)
            .map(|entry| blend(&entry.previous, &entry.current, alpha))
            .transpose()
    }

    pub(crate) fn clear(&mut self) {
        self.transforms.clear();
        self.visuals.clear();
        self.transform_slots.fill(None);
        self.visual_slots.fill(None);
        self.tick = Tick::ZERO;
    }
}

/// Copies transforms out of a world, one tick at a time.
#[derive(Debug)]
pub struct Extractor {
    capacity: usize,
    /// Last transforms indexed by NetId slot and guarded by generation.
    last: Vec<Option<(u32, Transform)>>,
    /// Build area for the next successful extract.
    scratch: Vec<Option<(u32, Transform)>>,
}

impl Extractor {
    /// Creates an extractor that can handle `capacity` entities without growing.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            last: vec![None; capacity],
            scratch: vec![None; capacity],
        }
    }

    /// The number of entities this extractor can copy without growing.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn reset(&mut self) {
        self.last.fill(None);
        self.scratch.fill(None);
    }

    /// Fills `out` (cleared first) with every live entity's transform and visual using dense
    /// world queries, then stamps it with `tick`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExtractCapacity`] when the world has more entities than `out` can
    /// hold. Slot tables grow only when a newly observed id extends the id space.
    pub fn extract(
        &mut self,
        world: &World,
        tick: Tick,
        out: &mut RenderFrame,
    ) -> Result<(), AppError> {
        let needed = world.len();
        if needed > out.capacity || needed > self.capacity {
            return Err(AppError::ExtractCapacity {
                capacity: out.capacity.min(self.capacity),
                needed,
            });
        }
        let slot_count = world.net_id_slot_count();
        if slot_count > out.transform_slots.len() {
            out.transform_slots.resize(slot_count, None);
            out.visual_slots.resize(slot_count, None);
        }
        if slot_count > self.last.len() {
            self.last.resize(slot_count, None);
            self.scratch.resize(slot_count, None);
        }
        out.transforms.clear();
        out.visuals.clear();
        out.transform_slots.fill(None);
        out.visual_slots.fill(None);
        self.scratch.fill(None);
        world.for_each1::<Transform>(|id, current| {
            let current = *current;
            let Ok(slot) = usize::try_from(id.index()) else {
                return;
            };
            let previous = self
                .last
                .get(slot)
                .and_then(Option::as_ref)
                .filter(|(generation, _)| *generation == id.generation())
                .map_or(current, |(_, transform)| *transform);
            let dense_index = out.transforms.len();
            out.transforms.push(ExtractedTransform {
                id,
                previous,
                current,
            });
            if let Some(frame_slot) = out.transform_slots.get_mut(slot) {
                *frame_slot = Some((id.generation(), dense_index));
            }
            if let Some(last_slot) = self.scratch.get_mut(slot) {
                *last_slot = Some((id.generation(), current));
            }
        });
        world.for_each2::<Transform, Visual>(|id, (_, visual)| {
            let dense_index = out.visuals.len();
            out.visuals.push(ExtractedVisual {
                id,
                visual: *visual,
            });
            if let Ok(slot) = usize::try_from(id.index())
                && let Some(frame_slot) = out.visual_slots.get_mut(slot)
            {
                *frame_slot = Some((id.generation(), dense_index));
            }
        });
        std::mem::swap(&mut self.last, &mut self.scratch);
        out.tick = tick;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use engine_scene::{Look, MaterialKind, MeshKind, register_components};
    use glam::{Quat, Vec3};

    fn at(x: f32) -> Transform {
        Transform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE).expect("valid")
    }

    fn frame(transforms: Vec<ExtractedTransform>, capacity: usize) -> RenderFrame {
        let mut frame = RenderFrame::with_capacity(capacity);
        for entry in transforms {
            let Ok(slot) = usize::try_from(entry.id.index()) else {
                continue;
            };
            if slot >= frame.transform_slots.len() {
                frame.transform_slots.resize(slot.saturating_add(1), None);
            }
            let dense_index = frame.transforms.len();
            frame.transform_slots[slot] = Some((entry.id.generation(), dense_index));
            frame.transforms.push(entry);
        }
        frame
    }

    #[test]
    fn blended_lies_between_previous_and_current() {
        let mut world = World::new(1).expect("world");
        let id = world.spawn().expect("spawn");
        let frame = frame(
            vec![ExtractedTransform {
                id,
                previous: at(0.0),
                current: at(2.0),
            }],
            4,
        );
        let mid = frame
            .blended(id, 0.5)
            .expect("valid alpha")
            .expect("present");
        assert!((mid.translation().x - 1.0).abs() < 1e-6);
        let start = frame
            .blended(id, 0.0)
            .expect("valid alpha")
            .expect("present");
        assert!(start.translation().x.abs() < 1e-6);
    }

    #[test]
    fn blended_finds_the_right_entity_among_several() {
        let mut world = World::new(2).expect("world");
        let ids: Vec<NetId> = (0..5).map(|_| world.spawn().expect("spawn")).collect();
        let transforms = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| ExtractedTransform {
                id,
                previous: at(i as f32),
                current: at(i as f32),
            })
            .collect();
        let frame = frame(transforms, 8);
        for (i, &id) in ids.iter().enumerate() {
            let found = frame.blended(id, 1.0).expect("valid").expect("present");
            assert!((found.translation().x - i as f32).abs() < 1e-6);
        }
    }

    #[test]
    fn blended_is_none_for_an_id_not_in_the_frame() {
        let mut world = World::new(3).expect("world");
        let a = world.spawn().expect("spawn");
        let b = world.spawn().expect("spawn");
        let c = world.spawn().expect("spawn");
        let frame = frame(
            vec![
                ExtractedTransform {
                    id: a,
                    previous: at(0.0),
                    current: at(0.0),
                },
                ExtractedTransform {
                    id: c,
                    previous: at(0.0),
                    current: at(0.0),
                },
            ],
            4,
        );
        assert!(frame.blended(b, 0.5).expect("valid alpha").is_none());
        let empty = RenderFrame::with_capacity(4);
        assert!(empty.blended(a, 0.5).expect("valid alpha").is_none());
    }

    #[test]
    fn blended_rejects_an_alpha_outside_the_unit_range() {
        let mut world = World::new(4).expect("world");
        let id = world.spawn().expect("spawn");
        let frame = frame(
            vec![ExtractedTransform {
                id,
                previous: at(0.0),
                current: at(1.0),
            }],
            4,
        );
        assert!(matches!(
            frame.blended(id, 1.5),
            Err(AppError::BlendAlpha(_))
        ));
    }

    fn crate_visual() -> Visual {
        Visual {
            mesh: MeshKind::Box,
            look: Look {
                material: MaterialKind::Crate,
                tint: None,
            },
        }
    }

    #[test]
    fn extract_lists_only_entities_that_carry_a_visual() {
        let mut world = World::new(5).expect("world");
        register_components(&mut world).expect("components");
        let plain = world.spawn().expect("spawn");
        let drawn = world.spawn().expect("spawn");
        world.insert(drawn, crate_visual()).expect("visual");
        let mut extractor = Extractor::with_capacity(4);
        let mut frame = RenderFrame::with_capacity(4);
        extractor
            .extract(&world, Tick::ZERO, &mut frame)
            .expect("extract");
        assert_eq!(frame.transforms().len(), 2);
        assert_eq!(frame.visuals().len(), 1);
        assert_eq!(frame.visuals()[0].id, drawn);
        assert_eq!(frame.visuals()[0].visual, crate_visual());
        assert!(frame.transform_entry(plain).is_some());
    }

    #[test]
    fn extract_works_when_no_visual_component_is_registered() {
        let mut world = World::new(6).expect("world");
        world.spawn().expect("spawn");
        let mut extractor = Extractor::with_capacity(2);
        let mut frame = RenderFrame::with_capacity(2);
        extractor
            .extract(&world, Tick::ZERO, &mut frame)
            .expect("extract without visuals");
        assert_eq!(frame.transforms().len(), 1);
        assert!(frame.visuals().is_empty());
    }

    #[test]
    fn reset_drops_old_world_slots_and_transform_history() {
        let mut old_world = World::new(7).expect("old world");
        register_components(&mut old_world).expect("components");
        let old_id = old_world.spawn().expect("old spawn");
        old_world
            .insert(old_id, crate_visual())
            .expect("old visual");
        let mut extractor = Extractor::with_capacity(2);
        let mut frame = RenderFrame::with_capacity(2);

        extractor
            .extract(&old_world, Tick::new(1), &mut frame)
            .expect("first old extract");
        old_world.insert(old_id, at(2.0)).expect("move old entity");
        extractor
            .extract(&old_world, Tick::new(2), &mut frame)
            .expect("second old extract");
        assert_eq!(
            frame.transform_entry(old_id).map(|entry| entry.previous),
            Some(at(0.0))
        );

        extractor.reset();
        frame.clear();
        assert!(frame.transform_entry(old_id).is_none());
        assert!(frame.visual_entry(old_id).is_none());

        let mut new_world = World::new(7).expect("new world");
        register_components(&mut new_world).expect("components");
        let new_id = new_world.spawn().expect("new spawn");
        new_world.insert(new_id, at(5.0)).expect("new position");
        extractor
            .extract(&new_world, Tick::new(3), &mut frame)
            .expect("first new extract");

        let entry = frame.transform_entry(new_id).expect("new transform");
        assert_eq!(entry.previous, at(5.0));
        assert_eq!(entry.current, at(5.0));
    }
}
