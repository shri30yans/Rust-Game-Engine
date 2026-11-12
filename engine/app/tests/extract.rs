//! Extraction copies transforms out of the world with the previous tick kept alongside.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{AppError, ExtractedTransform, Extractor, RenderFrame};
use engine_core::Tick;
use engine_world::{NetId, Transform, World};

/// A transform whose translation is `(x, 0, 0)`. Built from the identity so no glam import is needed.
fn at(x: f32) -> Transform {
    let mut transform = Transform::IDENTITY;
    let moved = transform.translation().with_x(x);
    transform
        .set_translation(moved)
        .expect("finite translation");
    transform
}

fn spawn_at(world: &mut World, x: f32) -> NetId {
    let id = world.spawn().expect("spawn");
    world.insert(id, at(x)).expect("insert transform");
    id
}

fn extract(
    extractor: &mut Extractor,
    world: &World,
    tick: u32,
    frame: &mut RenderFrame,
) -> Result<(), AppError> {
    extractor.extract(world, Tick::new(tick), frame)
}

fn find(frame: &RenderFrame, id: NetId) -> ExtractedTransform {
    *frame
        .transforms()
        .iter()
        .find(|entry| entry.id == id)
        .expect("entity present in frame")
}

#[test]
fn first_extract_has_previous_equal_to_current() {
    let mut world = World::new(1).expect("world");
    let a = spawn_at(&mut world, 1.0);
    let b = spawn_at(&mut world, 2.0);
    let c = spawn_at(&mut world, 3.0);
    let mut extractor = Extractor::with_capacity(8);
    let mut frame = RenderFrame::with_capacity(8);

    extract(&mut extractor, &world, 0, &mut frame).expect("extract");

    assert_eq!(frame.tick(), Tick::new(0));
    assert_eq!(frame.transforms().len(), 3);
    for id in [a, b, c] {
        let entry = find(&frame, id);
        assert_eq!(entry.previous, entry.current);
    }
}

#[test]
fn moved_entity_keeps_its_old_value_as_previous() {
    let mut world = World::new(2).expect("world");
    let a = spawn_at(&mut world, 1.0);
    let b = spawn_at(&mut world, 2.0);
    let c = spawn_at(&mut world, 3.0);
    let mut extractor = Extractor::with_capacity(8);
    let mut frame = RenderFrame::with_capacity(8);
    extract(&mut extractor, &world, 0, &mut frame).expect("first extract");

    world.insert(b, at(20.0)).expect("move b");
    extract(&mut extractor, &world, 1, &mut frame).expect("second extract");

    assert_eq!(frame.tick(), Tick::new(1));
    let moved = find(&frame, b);
    assert_eq!(moved.previous, at(2.0));
    assert_eq!(moved.current, at(20.0));
    for id in [a, c] {
        let still = find(&frame, id);
        assert_eq!(still.previous, still.current);
    }
}

#[test]
fn dense_transform_order_has_generation_checked_slot_lookups() {
    let mut world = World::new(3).expect("world");
    for x in 0..10u8 {
        spawn_at(&mut world, f32::from(x));
    }
    let mut extractor = Extractor::with_capacity(16);
    let mut frame = RenderFrame::with_capacity(16);

    extract(&mut extractor, &world, 0, &mut frame).expect("extract");

    let ids: Vec<NetId> = world.net_ids().collect();
    assert_eq!(frame.transforms().len(), ids.len());
    for (index, id) in ids.into_iter().enumerate() {
        let entry = frame.transform_entry(id).expect("live id resolves");
        assert_eq!(entry.current, at(index as f32));
    }
}

#[test]
fn entity_spawned_between_extracts_has_previous_equal_to_current() {
    let mut world = World::new(4).expect("world");
    let a = spawn_at(&mut world, 1.0);
    let mut extractor = Extractor::with_capacity(8);
    let mut frame = RenderFrame::with_capacity(8);
    extract(&mut extractor, &world, 0, &mut frame).expect("first extract");

    let late = spawn_at(&mut world, 9.0);
    extract(&mut extractor, &world, 1, &mut frame).expect("second extract");

    let newcomer = find(&frame, late);
    assert_eq!(newcomer.previous, newcomer.current);
    assert_eq!(newcomer.current, at(9.0));
    let existing = find(&frame, a);
    assert_eq!(existing.previous, existing.current);
}

#[test]
fn despawned_entity_vanishes_from_the_frame() {
    let mut world = World::new(5).expect("world");
    let a = spawn_at(&mut world, 1.0);
    let b = spawn_at(&mut world, 2.0);
    let mut extractor = Extractor::with_capacity(8);
    let mut frame = RenderFrame::with_capacity(8);
    extract(&mut extractor, &world, 0, &mut frame).expect("first extract");

    assert_eq!(world.despawn(b).expect("despawn"), 1);
    extract(&mut extractor, &world, 1, &mut frame).expect("second extract");

    assert_eq!(frame.transforms().len(), 1);
    assert_eq!(frame.transforms().first().map(|entry| entry.id), Some(a));
}

#[test]
fn capacity_overflow_is_an_error_and_leaves_the_extractor_usable() {
    let mut world = World::new(6).expect("world");
    let a = spawn_at(&mut world, 1.0);
    let b = spawn_at(&mut world, 2.0);
    let c = spawn_at(&mut world, 3.0);
    let mut extractor = Extractor::with_capacity(8);
    let mut frame = RenderFrame::with_capacity(3);
    extract(&mut extractor, &world, 0, &mut frame).expect("tick 0 fits");

    // Two more entities: five in the world, room for three in the frame.
    let d = spawn_at(&mut world, 4.0);
    let e = spawn_at(&mut world, 5.0);
    let overflow = extract(&mut extractor, &world, 1, &mut frame);
    assert!(matches!(
        overflow,
        Err(AppError::ExtractCapacity {
            capacity: 3,
            needed: 5
        })
    ));
    assert_eq!(
        frame.tick(),
        Tick::new(0),
        "failed extract must not stamp the frame"
    );
    assert_eq!(
        frame.transforms().len(),
        3,
        "failed extract must not touch the frame"
    );

    // Back under the limit, and the change made before the failed call is seen against tick 0.
    world.despawn(d).expect("despawn d");
    world.despawn(e).expect("despawn e");
    world.insert(a, at(10.0)).expect("move a");
    extract(&mut extractor, &world, 2, &mut frame).expect("tick 2 fits");

    let moved = find(&frame, a);
    assert_eq!(
        moved.previous,
        at(1.0),
        "last must hold tick 0, not the failed call"
    );
    assert_eq!(moved.current, at(10.0));
    assert_eq!(find(&frame, b).previous, at(2.0));
    assert_eq!(find(&frame, c).previous, at(3.0));
}

#[test]
fn extractor_capacity_limits_the_extract_too() {
    let mut world = World::new(7).expect("world");
    spawn_at(&mut world, 1.0);
    spawn_at(&mut world, 2.0);
    let mut extractor = Extractor::with_capacity(1);
    let mut frame = RenderFrame::with_capacity(8);

    let result = extract(&mut extractor, &world, 0, &mut frame);

    assert!(matches!(
        result,
        Err(AppError::ExtractCapacity { needed: 2, .. })
    ));
    assert!(frame.transforms().is_empty());
}

#[test]
fn render_frame_reports_its_fixed_capacity() {
    let frame = RenderFrame::with_capacity(5);
    assert!(frame.capacity() >= 5);
    assert_eq!(frame.tick(), Tick::ZERO);
    assert!(frame.transforms().is_empty());
}
