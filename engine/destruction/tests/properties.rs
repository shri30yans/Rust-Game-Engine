//! Property tests over random support graphs with random damage and random budgets.
//!
//! Checked invariants: mass is conserved at every step, `Broken` and `Detached`
//! are terminal, the per-step visit budget holds, every settled intact element
//! reaches an anchor, the state hash is deterministic, and budgeted cycles settle
//! to the same state as their unbudgeted equivalents even with mid-cycle damage.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use std::collections::{BTreeSet, VecDeque};
use std::num::NonZeroU32;
use std::sync::Arc;

use engine_core::Tick;
use engine_destruction::{DestructionBudget, ElementState, EventSink, Structure, StructureState};
use proptest::prelude::*;

const UNBUDGETED: u32 = 1_000_000;
const STEP_LIMIT: u32 = 200_000;

#[derive(Debug, Clone)]
struct Scenario {
    caps: Vec<f32>,
    parents: Vec<usize>,
    extra: Vec<(usize, usize)>,
    damage: Vec<(usize, u16)>,
    during_damage: Vec<(usize, u16)>,
    warning: u32,
}

fn scenario() -> impl Strategy<Value = Scenario> {
    (2usize..=24).prop_flat_map(|n| {
        (
            prop::collection::vec(0.5f32..6.0, n),
            prop::collection::vec(0usize..10_000, n),
            prop::collection::vec((0..n, 0..n), 0..6),
            prop::collection::vec((0..n, 0u16..=120), 0..4),
            prop::collection::vec((0..n, 1u16..=120), 1..=4),
            1u32..=4,
        )
            .prop_map(
                |(caps, parents, extra, damage, during_damage, warning)| Scenario {
                    caps,
                    parents,
                    extra,
                    damage,
                    during_damage,
                    warning,
                },
            )
    })
}

/// Builds the structure: a random tree rooted at element 0, plus extra edges.
/// Element 0 and every fifth element are anchors, so every element is supported at build.
fn structure_of(s: &Scenario) -> Structure {
    let n = s.caps.len();
    let anchors: Vec<usize> = (0..n).filter(|i| i % 5 == 0).collect();
    let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
    for i in 1..n {
        edges.insert((s.parents[i] % i, i));
    }
    for &(a, b) in &s.extra {
        if a != b {
            edges.insert((a.min(b), a.max(b)));
        }
    }
    let edges: Vec<(usize, usize)> = edges.into_iter().collect();
    common::build(&s.caps, &anchors, &edges, s.warning)
}

/// Queues a hit list in its generated order.
fn queue_damage(
    state: &mut StructureState,
    damage: &[(usize, u16)],
    tick: Tick,
    sink: &mut EventSink,
) {
    for &(index, amount) in damage {
        state
            .apply_damage(common::id(index), amount, tick, sink)
            .unwrap();
    }
}

/// Finishes a state with the given per-step visit budget, checking per-step invariants.
fn finish(mut state: StructureState, visits: u32, sink: &mut EventSink) -> StructureState {
    let n = state.structure().len();
    let budget = DestructionBudget::new(NonZeroU32::new(visits).unwrap());
    let mut previous: Vec<ElementState> = (0..n)
        .map(|i| state.state(common::id(i)).unwrap())
        .collect();

    for _ in 0..STEP_LIMIT {
        let report = state.step(Tick::ZERO, sink, budget).unwrap();
        assert!(
            report.visits <= usize::try_from(visits).unwrap(),
            "step exceeded its budget"
        );

        let totals = state.mass_totals();
        assert_eq!(
            totals.supported + totals.broken + totals.detached,
            n as f32,
            "mass not conserved"
        );

        for (i, seen) in previous.iter_mut().enumerate() {
            let now = state.state(common::id(i)).unwrap();
            if *seen == ElementState::Broken {
                assert_eq!(now, ElementState::Broken, "Broken element {i} changed");
            }
            if *seen == ElementState::Detached {
                assert_eq!(now, ElementState::Detached, "Detached element {i} changed");
            }
            *seen = now;
        }

        if report.settled {
            return state;
        }
    }
    panic!("state did not settle within {STEP_LIMIT} steps");
}

/// Runs `s` from construction through settlement with one visit budget.
fn run(s: &Scenario, visits: u32) -> StructureState {
    let mut state = StructureState::new(Arc::new(structure_of(s)));
    let mut sink = EventSink::with_capacity(4096);
    queue_damage(&mut state, &s.damage, Tick::ZERO, &mut sink);
    finish(state, visits, &mut sink)
}

/// Table index of an element id.
fn idx(id: engine_destruction::ElementId) -> usize {
    usize::try_from(id.raw()).unwrap()
}

/// Live elements reachable from the anchors through live elements.
fn supported_by_anchor(state: &StructureState) -> Vec<bool> {
    let structure = state.structure();
    let live = |id| state.state(id).is_some_and(|s| s.is_live());
    let mut reached = vec![false; structure.len()];
    let mut queue = VecDeque::new();
    for &anchor in structure.anchors() {
        if live(anchor) {
            reached[idx(anchor)] = true;
            queue.push_back(anchor);
        }
    }
    while let Some(current) = queue.pop_front() {
        for &next in structure.neighbours(current) {
            if !reached[idx(next)] && live(next) {
                reached[idx(next)] = true;
                queue.push_back(next);
            }
        }
    }
    reached
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn settled_intact_elements_are_connected_to_an_anchor(s in scenario()) {
        let state = run(&s, UNBUDGETED);
        prop_assert!(state.is_settled());
        let reached = supported_by_anchor(&state);
        for (i, &is_reached) in reached.iter().enumerate() {
            if state.state(common::id(i)) == Some(ElementState::Intact) {
                prop_assert!(is_reached, "intact element {i} has no path to an anchor");
            }
        }
    }

    #[test]
    fn state_hash_is_deterministic(s in scenario(), visits in 1u32..=16) {
        let first = run(&s, visits);
        let second = run(&s, visits);
        prop_assert_eq!(first.state_hash(), second.state_hash());
    }

    #[test]
    fn budgeted_run_settles_to_the_unbudgeted_state(s in scenario(), visits in 1u32..=16) {
        let reference = run(&s, UNBUDGETED);
        let budgeted = run(&s, visits);
        prop_assert_eq!(budgeted.state_hash(), reference.state_hash());
        prop_assert_eq!(budgeted.groups(), reference.groups());
        prop_assert_eq!(budgeted.detached_members(), reference.detached_members());
    }

    #[test]
    fn damage_between_budgeted_steps_waits_for_the_next_cycle(
        s in scenario(),
        visits in 1u32..=16,
    ) {
        let structure = Arc::new(structure_of(&s));
        let mut budgeted = StructureState::new(Arc::clone(&structure));
        let mut budgeted_sink = EventSink::with_capacity(4096);
        queue_damage(&mut budgeted, &s.damage, Tick::ZERO, &mut budgeted_sink);

        let one_visit = DestructionBudget::new(NonZeroU32::new(1).unwrap());
        let first = budgeted
            .step(Tick::ZERO, &mut budgeted_sink, one_visit)
            .unwrap();
        prop_assert_eq!(first.visits, 1);
        prop_assert!(!first.settled);
        queue_damage(
            &mut budgeted,
            &s.during_damage,
            Tick::new(1),
            &mut budgeted_sink,
        );
        let budgeted = finish(budgeted, visits, &mut budgeted_sink);

        let mut reference = StructureState::new(structure);
        let mut reference_sink = EventSink::with_capacity(4096);
        queue_damage(&mut reference, &s.damage, Tick::ZERO, &mut reference_sink);
        let mut reference = finish(reference, UNBUDGETED, &mut reference_sink);
        queue_damage(
            &mut reference,
            &s.during_damage,
            Tick::new(1),
            &mut reference_sink,
        );
        let reference = finish(reference, UNBUDGETED, &mut reference_sink);

        prop_assert_eq!(budgeted.state_hash(), reference.state_hash());
        prop_assert_eq!(budgeted.groups(), reference.groups());
        prop_assert_eq!(budgeted.detached_members(), reference.detached_members());
    }
}
