//! Hand-computed scenarios for the load solver and the collapse countdown.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use std::num::NonZeroU32;
use std::sync::Arc;

use engine_core::Tick;
use engine_destruction::{
    DestructionBudget, DestructionEvent, ElementState, EventSink, StructureState,
};

use common::id;

fn unlimited() -> DestructionBudget {
    DestructionBudget::new(NonZeroU32::new(1_000_000).unwrap())
}

#[test]
fn column_with_base_overloaded_breaks_and_the_column_above_falls_as_one_group() {
    // Three unit-mass elements in a column. The base carries a load of 3 on capacity 2.5.
    let structure = Arc::new(common::build(&[2.5, 3.0, 3.0], &[0], &[(0, 1), (1, 2)], 2));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);
    common::settle(&mut state, &mut sink, 1_000_000);

    assert_eq!(state.state(id(0)), Some(ElementState::Broken));
    assert_eq!(state.state(id(1)), Some(ElementState::Detached));
    assert_eq!(state.state(id(2)), Some(ElementState::Detached));
    assert_eq!(state.groups().len(), 1);
    assert_eq!(state.groups()[0].mass(), 2.0);
    assert_eq!(state.detached_members(), &[id(1), id(2)]);
}

#[test]
fn removing_a_middle_column_detaches_the_elements_beyond_it_as_one_group() {
    // Chain A-B-C-D with A anchored. Removing B leaves C and D with no anchor.
    let structure = Arc::new(common::build(
        &[10.0; 4],
        &[0],
        &[(0, 1), (1, 2), (2, 3)],
        2,
    ));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);
    common::settle(&mut state, &mut sink, 1_000_000);

    state
        .apply_damage(id(1), 100, Tick::ZERO, &mut sink)
        .unwrap();
    common::settle(&mut state, &mut sink, 1_000_000);

    assert_eq!(state.state(id(0)), Some(ElementState::Intact));
    assert_eq!(state.state(id(1)), Some(ElementState::Broken));
    assert_eq!(state.state(id(2)), Some(ElementState::Detached));
    assert_eq!(state.state(id(3)), Some(ElementState::Detached));
    assert_eq!(state.groups().len(), 1);
    assert_eq!(state.groups()[0].mass(), 2.0);
    let detached = sink
        .events()
        .iter()
        .any(|event| matches!(event.kind, DestructionEvent::GroupDetached { .. }));
    assert!(detached, "expected a GroupDetached event");
}

#[test]
fn warning_phase_lasts_exactly_min_warning_ticks_rounds() {
    const WARNING: u32 = 5;
    // Anchor A with a light child B. A carries a load of 2 on capacity 1 and fails at once.
    let structure = Arc::new(common::build(&[1.0, 10.0], &[0], &[(0, 1)], WARNING));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);

    let mut failing_at = None;
    let mut broken_at = None;
    for step in 1..=20u32 {
        state.step(Tick::ZERO, &mut sink, unlimited()).unwrap();
        match state.state(id(0)) {
            Some(ElementState::Failing { ticks_left }) if failing_at.is_none() => {
                assert_eq!(ticks_left, WARNING);
                failing_at = Some(step);
            }
            Some(ElementState::Broken) if broken_at.is_none() => broken_at = Some(step),
            _ => {}
        }
        if broken_at.is_some() {
            break;
        }
    }

    let failing_at = failing_at.expect("anchor should start failing");
    let broken_at = broken_at.expect("anchor should break");
    assert_eq!(broken_at - failing_at, WARNING);
}

#[test]
fn loads_of_a_branching_support_tree_pick_out_the_overloaded_elements() {
    // Anchor A supports B and C. B supports D. Unit masses give loads D=1, B=2, C=1, A=4.
    // Capacities are chosen so only A (4 > 3.5) and C (1 > 0.5) are overloaded.
    let structure = Arc::new(common::build(
        &[3.5, 2.5, 0.5, 1.0],
        &[0],
        &[(0, 1), (0, 2), (1, 3)],
        3,
    ));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);
    state.step(Tick::ZERO, &mut sink, unlimited()).unwrap();

    assert_eq!(
        state.state(id(0)),
        Some(ElementState::Failing { ticks_left: 3 })
    );
    assert_eq!(state.state(id(1)), Some(ElementState::Intact));
    assert_eq!(
        state.state(id(2)),
        Some(ElementState::Failing { ticks_left: 3 })
    );
    assert_eq!(state.state(id(3)), Some(ElementState::Intact));
}

#[test]
fn tower_base_damage_drops_the_column_and_leaves_neighbours_supported() {
    // Two ground-anchored towers of height 4. Damaging tower 0's base removes its support.
    let structure = Arc::new(common::ground_towers(2, 4, 2));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(256);
    common::settle(&mut state, &mut sink, 1_000_000);
    assert!(state.groups().is_empty(), "intact towers should not fall");

    state
        .apply_damage(id(0), 100, Tick::ZERO, &mut sink)
        .unwrap();
    common::settle(&mut state, &mut sink, 1_000_000);

    assert_eq!(state.state(id(0)), Some(ElementState::Broken));
    for floor in 1..4 {
        assert_eq!(state.state(id(floor)), Some(ElementState::Detached));
    }
    assert_eq!(state.groups().len(), 1);
    assert_eq!(state.groups()[0].mass(), 3.0);
    for floor in 0..4 {
        assert_eq!(state.state(id(4 + floor)), Some(ElementState::Intact));
    }
}

#[test]
fn damage_to_a_broken_element_is_ignored() {
    let structure = Arc::new(common::build(&[10.0, 10.0], &[0], &[(0, 1)], 1));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(64);
    state
        .apply_damage(id(1), 100, Tick::ZERO, &mut sink)
        .unwrap();
    common::settle(&mut state, &mut sink, 1_000_000);
    assert_eq!(state.state(id(1)), Some(ElementState::Broken));

    let before = state.state_hash();
    state
        .apply_damage(id(1), 50, Tick::ZERO, &mut sink)
        .unwrap();
    assert_eq!(state.state_hash(), before);
    assert_eq!(state.health(id(1)), Some(0));
}
