//! Cycle-boundary damage application, merging, event order, and initial scheduling.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use std::num::NonZeroU32;
use std::sync::Arc;

use engine_core::Tick;
use engine_destruction::{
    DestructionBudget, DestructionEvent, ElementState, EventSink, HealthBand, StructureState,
};

use common::id;

fn unlimited() -> DestructionBudget {
    DestructionBudget::new(NonZeroU32::new(1_000_000).unwrap())
}

#[test]
fn pre_step_damage_applies_before_the_initial_solve() {
    // The base carries exactly its undamaged capacity, but one damage point overloads it.
    let structure = Arc::new(common::build(&[2.0, 10.0], &[0], &[(0, 1)], 3));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);

    state
        .apply_damage(id(0), 1, Tick::new(7), &mut sink)
        .unwrap();
    assert_eq!(state.health(id(0)), Some(100));
    assert!(sink.is_empty());

    let report = state.step(Tick::new(8), &mut sink, unlimited()).unwrap();

    assert!(!report.settled);
    assert_eq!(state.health(id(0)), Some(99));
    assert_eq!(
        state.state(id(0)),
        Some(ElementState::Failing { ticks_left: 3 })
    );
    assert!(matches!(
        sink.events().first(),
        Some(event)
            if event.tick == Tick::new(7)
                && matches!(
                    event.kind,
                    DestructionEvent::ElementDamaged {
                        id: damaged,
                        band: HealthBand::Healthy,
                    } if damaged == id(0)
                )
    ));
}

#[test]
fn pending_hits_merge_and_emit_in_element_id_order() {
    let structure = Arc::new(common::build(
        &[10.0, 10.0, 10.0],
        &[0],
        &[(0, 1), (1, 2)],
        2,
    ));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);
    common::settle(&mut state, &mut sink, 1_000_000);
    sink.clear();

    state
        .apply_damage(id(2), 10, Tick::new(7), &mut sink)
        .unwrap();
    state
        .apply_damage(id(0), 5, Tick::new(8), &mut sink)
        .unwrap();
    state
        .apply_damage(id(2), 20, Tick::new(9), &mut sink)
        .unwrap();

    assert!(sink.is_empty());
    assert_eq!(state.health(id(0)), Some(100));
    assert_eq!(state.health(id(2)), Some(100));

    common::settle(&mut state, &mut sink, 1_000_000);

    assert_eq!(state.health(id(0)), Some(95));
    assert_eq!(state.health(id(2)), Some(70));
    assert_eq!(sink.events().len(), 2);
    assert!(matches!(
        sink.events().first(),
        Some(event)
            if event.tick == Tick::new(8)
                && matches!(
                    event.kind,
                    DestructionEvent::ElementDamaged { id: damaged, .. }
                        if damaged == id(0)
                )
    ));
    assert!(matches!(
        sink.events().get(1),
        Some(event)
            if event.tick == Tick::new(7)
                && matches!(
                    event.kind,
                    DestructionEvent::ElementDamaged { id: damaged, .. }
                        if damaged == id(2)
                )
    ));
}
