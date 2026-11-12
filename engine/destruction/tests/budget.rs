//! Budget behaviour on a multi-tower structure: per-step visits stay within the
//! budget, and a tight budget settles to the same result as an unlimited one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;

use std::num::NonZeroU32;
use std::sync::Arc;

use engine_core::Tick;
use engine_destruction::{DestructionBudget, ElementState, EventSink, StructureState};

fn damaged_towers(visits: u32) -> (StructureState, u32, usize) {
    let structure = Arc::new(common::ground_towers(6, 12, 3));
    let mut state = StructureState::new(structure);
    let mut sink = EventSink::with_capacity(4096);
    state
        .apply_damage(common::id(0), 100, Tick::ZERO, &mut sink)
        .unwrap();

    let budget = DestructionBudget::new(NonZeroU32::new(visits).unwrap());
    let mut steps = 0u32;
    let mut peak = 0usize;
    loop {
        let report = state.step(Tick::ZERO, &mut sink, budget).unwrap();
        steps += 1;
        peak = peak.max(report.visits);
        if report.settled {
            break;
        }
        assert!(steps < 1_000_000, "did not settle");
    }
    (state, steps, peak)
}

#[test]
fn tight_budget_never_exceeds_its_visits_per_step() {
    let (_, _, peak) = damaged_towers(5);
    assert!(peak <= 5, "peak visits {peak} exceeded budget 5");
}

#[test]
fn tight_budget_takes_more_steps_but_reaches_the_same_result() {
    let (fast, fast_steps, _) = damaged_towers(1_000_000);
    let (slow, slow_steps, _) = damaged_towers(4);

    assert!(
        slow_steps > fast_steps,
        "a smaller budget should take longer"
    );
    assert_eq!(slow.state_hash(), fast.state_hash());
    assert_eq!(slow.groups(), fast.groups());
    assert_eq!(slow.state(common::id(0)), Some(ElementState::Broken));
    assert_eq!(fast.groups().len(), 1);
    assert_eq!(fast.groups()[0].mass(), 11.0);
}
