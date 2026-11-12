//! Determinism: the same seed and systems give the same per-tick state hashes,
//! and each system's RNG stream is reproducible and independent of the others.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_core::Tick;
use engine_world::{
    NetId, Replication, Schedule, SystemContext, Transform, World, WorldError, propagate_transforms,
};
use glam::Vec3;
use serde::{Deserialize, Serialize};

const TICKS: u32 = 1000;
const SEED: u64 = 0x00C0_FFEE;
const DT: f32 = 1.0 / 60.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Health {
    hp: u32,
}

/// Moves about half the entities by a random offset.
fn wander(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let ids: Vec<NetId> = world.net_ids().collect();
    for id in ids {
        if ctx.rng.next_f32() < 0.5 {
            let offset = Vec3::new(
                ctx.rng.range_f32(-1.0, 1.0)?,
                ctx.rng.range_f32(-1.0, 1.0)?,
                ctx.rng.range_f32(-1.0, 1.0)?,
            );
            let mut transform = world.get_mut::<Transform>(id)?;
            let next = transform.translation() + offset * ctx.dt;
            transform.set_translation(next)?;
        }
    }
    Ok(())
}

/// Spawns, despawns and reparents at random. Cycle rejections are expected and ignored.
fn churn(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let ids: Vec<NetId> = world.net_ids().collect();
    let roll = ctx.rng.next_f32();
    if roll < 0.08 || ids.is_empty() {
        let id = world.spawn()?;
        let hp = ctx.rng.range_u32(1, 100)?;
        world.insert(id, Health { hp })?;
    } else if roll < 0.12 {
        if let Some(victim) = pick(ctx, &ids)? {
            world.despawn(victim)?;
        }
    } else if roll < 0.95 {
        if let (Some(child), Some(parent)) = (pick(ctx, &ids)?, pick(ctx, &ids)?) {
            match world.set_parent(child, parent) {
                Ok(()) | Err(WorldError::Cycle { .. }) => {}
                Err(other) => return Err(other),
            }
        }
    } else if let Some(id) = pick(ctx, &ids)? {
        world.detach(id)?;
    }
    Ok(())
}

fn pick(ctx: &mut SystemContext<'_>, ids: &[NetId]) -> Result<Option<NetId>, WorldError> {
    if ids.is_empty() {
        return Ok(None);
    }
    let bound = u32::try_from(ids.len()).map_err(|_| WorldError::Invariant("id list too long"))?;
    let raw = ctx.rng.range_u32(0, bound)?;
    let index = usize::try_from(raw).map_err(|_| WorldError::Invariant("index overflow"))?;
    Ok(ids.get(index).copied())
}

fn build_schedule(world: &World) -> Schedule {
    let mut schedule = Schedule::new(world.rng(), DT).unwrap();
    schedule.add_system("wander", wander).unwrap();
    schedule.add_system("churn", churn).unwrap();
    schedule
        .add_system("propagate_transforms", propagate_transforms)
        .unwrap();
    schedule
}

fn run_trace() -> Vec<u64> {
    let mut world = World::new(SEED).unwrap();
    world.register::<Health>(Replication::Replicated).unwrap();
    for _ in 0..32 {
        world.spawn().unwrap();
    }
    let mut schedule = build_schedule(&world);
    let mut trace = Vec::with_capacity(TICKS as usize);
    let mut tick = Tick::ZERO;
    for _ in 0..TICKS {
        schedule.run_tick(&mut world, tick).unwrap();
        trace.push(world.state_hash().unwrap());
        tick = tick.next();
    }
    trace
}

#[test]
fn thousand_ticks_twice_give_identical_hash_traces() {
    let first = run_trace();
    let second = run_trace();
    assert_eq!(first.len(), TICKS as usize);
    assert_eq!(first, second);
    // The trace must actually move; a constant hash would make the comparison meaningless.
    let mut distinct = first.clone();
    distinct.sort_unstable();
    distinct.dedup();
    // Repeats are legal when a tick changes nothing (e.g. a tiny world where no entity moves),
    // so require a clear majority of distinct states rather than near-total uniqueness.
    assert!(
        distinct.len() > TICKS as usize / 2,
        "only {} distinct hashes",
        distinct.len()
    );
}

fn sample_a(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let id = world.spawn()?;
    world.insert(
        id,
        Health {
            hp: ctx.rng.next_u32(),
        },
    )
}

fn sample_b(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let id = world.spawn()?;
    world.insert(
        id,
        Health {
            hp: ctx.rng.next_u32(),
        },
    )
}

/// Runs eight ticks where each system spawns one entity holding a drawn value.
/// Returns the values in spawn order, so the output alternates by schedule order.
fn draws(order: [&'static str; 2]) -> Vec<u32> {
    let mut world = World::new(SEED).unwrap();
    world.register::<Health>(Replication::Replicated).unwrap();
    let mut schedule = Schedule::new(world.rng(), DT).unwrap();
    for name in order {
        let run: engine_world::SystemFn = if name == "a" { sample_a } else { sample_b };
        schedule.add_system(name, run).unwrap();
    }
    for tick in 0..8u32 {
        schedule.run_tick(&mut world, Tick::new(tick)).unwrap();
    }
    world
        .net_ids()
        .map(|id| world.get::<Health>(id).unwrap().hp)
        .collect()
}

fn evens(values: &[u32]) -> Vec<u32> {
    values.iter().step_by(2).copied().collect()
}

fn odds(values: &[u32]) -> Vec<u32> {
    values.iter().skip(1).step_by(2).copied().collect()
}

#[test]
fn per_system_streams_are_reproducible_and_independent() {
    let a_then_b = draws(["a", "b"]);
    let b_then_a = draws(["b", "a"]);
    assert_eq!(a_then_b.len(), 16);
    // Reproducible: the same schedule draws the same values.
    assert_eq!(draws(["a", "b"]), a_then_b);
    // Keyed by name, not position: "a" draws the same values whichever slot it occupies.
    assert_eq!(evens(&a_then_b), odds(&b_then_a));
    assert_eq!(odds(&a_then_b), evens(&b_then_a));
    // Independent: the two streams differ.
    assert_ne!(evens(&a_then_b), odds(&a_then_b));
}
