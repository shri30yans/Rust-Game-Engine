//! Criterion benches for engine-destruction: one budgeted step on a 10k-element
//! building, and the full settle after removing a ground-floor column.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::num::NonZeroU32;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use engine_core::Tick;
use engine_destruction::{DestructionBudget, ElementId, EventSink, StructureState};

const BUDGET: u32 = 2048;
const TOWERS: usize = 100;
const HEIGHT: usize = 100;

fn bench_destruction(c: &mut Criterion) {
    let structure = Arc::new(common::ground_towers(TOWERS, HEIGHT, 3));
    let budget = DestructionBudget::new(NonZeroU32::new(BUDGET).unwrap());
    let mut group = c.benchmark_group("destruction_10k");

    group.bench_function("step_budget_2048_during_initial_solve", |b| {
        b.iter_batched(
            || {
                (
                    StructureState::new(Arc::clone(&structure)),
                    EventSink::with_capacity(1 << 16),
                )
            },
            |(mut state, mut sink)| {
                let report = state.step(Tick::ZERO, &mut sink, budget).unwrap();
                black_box(report)
            },
            BatchSize::LargeInput,
        )
    });

    group.bench_function("settle_after_ground_column_removal", |b| {
        b.iter_batched(
            || {
                let mut state = StructureState::new(Arc::clone(&structure));
                let mut sink = EventSink::with_capacity(1 << 16);
                common::settle(&mut state, &mut sink, BUDGET);
                state
                    .apply_damage(ElementId::new(0), 100, Tick::ZERO, &mut sink)
                    .unwrap();
                sink.clear();
                (state, sink)
            },
            |(mut state, mut sink)| {
                let mut steps = 0u32;
                loop {
                    let report = state.step(Tick::ZERO, &mut sink, budget).unwrap();
                    steps += 1;
                    if report.settled {
                        break;
                    }
                }
                black_box(steps)
            },
            BatchSize::LargeInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_destruction);
criterion_main!(benches);
