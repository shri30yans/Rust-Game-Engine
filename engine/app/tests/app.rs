//! App behaviour: ticks, per-tick state hashes, seed sensitivity, worker-count
//! independence, device-profile sizing and the inline web background pool.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::num::NonZeroUsize;

use app::{App, AppConfig, AppError, Game, GameInfo, TickRate, run_headless};
use engine_world::{NetId, Replication, Schedule, SystemContext, World, WorldError};
use platform::{CpuProfile, DeviceClass, DeviceProfile};
use serde::{Deserialize, Serialize};

/// Counts how many ticks have run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Counter(u32);

/// Accumulates one random number per tick.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Noise(u32);

fn tick_counter(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let ids: Vec<NetId> = world.net_ids().collect();
    for id in ids {
        let draw = ctx.rng.next_u32();
        {
            let mut counter = world.get_mut::<Counter>(id)?;
            counter.0 = counter.0.wrapping_add(1);
        }
        let mut noise = world.get_mut::<Noise>(id)?;
        noise.0 = noise.0.wrapping_add(draw);
    }
    Ok(())
}

struct TestGame;

impl Game for TestGame {
    fn dynamic_entity_budget(&self) -> usize {
        0
    }

    type Command = ();
    type Settings = ();

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "test-game",
            protocol_id: 0x7e57,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        _: &Self::Settings,
    ) -> Result<(), AppError> {
        world.register::<Counter>(Replication::Replicated)?;
        world.register::<Noise>(Replication::Replicated)?;
        let id = world.spawn()?;
        world.insert(id, Counter(0))?;
        world.insert(id, Noise(0))?;
        schedule.add_system("tick_counter", tick_counter)?;
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        _: app::PlayerId,
        _: &engine_sessions::PlayerName,
    ) -> Result<NetId, AppError> {
        if let Some(id) = world.net_ids().next() {
            Ok(id)
        } else {
            Ok(world.spawn()?)
        }
    }

    fn leave(&self, _: &mut World, _: app::PlayerId) -> Result<(), AppError> {
        Ok(())
    }

    fn command(&self, _: &mut World, _: app::PlayerId, _: &()) -> Result<(), AppError> {
        Ok(())
    }
}

fn hand_profile(class: DeviceClass) -> DeviceProfile {
    DeviceProfile {
        class,
        touch: class == DeviceClass::Handheld,
        cpu: CpuProfile {
            logical_cores: NonZeroUsize::new(4).expect("non-zero"),
            performance_cores: NonZeroUsize::new(4).expect("non-zero"),
        },
        memory_bytes: None,
        gpu: None,
    }
}

fn config(seed: u64, job_workers: Option<usize>) -> AppConfig {
    AppConfig {
        tick_rate: TickRate::new(60).expect("valid rate"),
        seed,
        max_catch_up_ticks: 8,
        job_workers,
    }
}

fn hashes(seed: u64, ticks: u32, job_workers: Option<usize>) -> Vec<u64> {
    let profile = hand_profile(DeviceClass::Desktop);
    let mut app =
        App::new(TestGame, &(), &config(seed, job_workers), &profile).expect("app builds");
    run_headless(&mut app, ticks).expect("headless run")
}

fn counter_value(app: &App<TestGame>) -> u32 {
    let id = app.world().net_ids().next().expect("one entity");
    app.world().get::<Counter>(id).expect("counter").0
}

#[test]
fn step_ten_times_runs_ten_ticks() {
    let profile = hand_profile(DeviceClass::Desktop);
    let mut app = App::new(TestGame, &(), &config(3, Some(0)), &profile).expect("app builds");
    for _ in 0..10 {
        app.step().expect("step");
    }
    assert_eq!(app.tick().raw(), 10);
    assert_eq!(counter_value(&app), 10);
}

#[test]
fn advance_with_three_steps_runs_three_ticks() {
    let profile = hand_profile(DeviceClass::Desktop);
    let cfg = config(3, Some(0));
    let mut app = App::new(TestGame, &(), &cfg, &profile).expect("app builds");
    let step = cfg.tick_rate.step_nanos();
    let advance = app.advance(3 * step).expect("advance");
    assert_eq!(advance.ticks, 3);
    assert_eq!(app.tick().raw(), 3);
    assert_eq!(counter_value(&app), 3);
}

#[test]
fn same_seed_and_ticks_give_identical_hashes_across_runs() {
    assert_eq!(hashes(7, 40, Some(0)), hashes(7, 40, Some(0)));
}

#[test]
fn different_seed_gives_different_hashes() {
    assert_ne!(hashes(7, 40, Some(0)), hashes(8, 40, Some(0)));
}

#[test]
fn run_headless_returns_one_hash_per_tick_and_last_matches_state_hash() {
    let profile = hand_profile(DeviceClass::Desktop);
    let mut app = App::new(TestGame, &(), &config(5, Some(0)), &profile).expect("app builds");
    let run = run_headless(&mut app, 25).expect("headless run");
    assert_eq!(run.len(), 25);
    assert_eq!(run.last().copied(), Some(app.state_hash().expect("hash")));
}

#[test]
fn run_headless_zero_ticks_is_empty() {
    let profile = hand_profile(DeviceClass::Desktop);
    let mut app = App::new(TestGame, &(), &config(5, Some(0)), &profile).expect("app builds");
    assert!(run_headless(&mut app, 0).expect("headless run").is_empty());
    assert_eq!(app.tick().raw(), 0);
}

#[test]
fn hashes_are_identical_for_zero_one_and_four_workers() {
    let zero = hashes(11, 50, Some(0));
    let one = hashes(11, 50, Some(1));
    let four = hashes(11, 50, Some(4));
    assert_eq!(zero, one);
    assert_eq!(zero, four);
}

#[test]
fn job_workers_none_uses_the_profile_count() {
    let profile = DeviceProfile::server().expect("server profile");
    let app = App::new(TestGame, &(), &config(1, None), &profile).expect("app builds");
    assert_eq!(app.jobs().workers(), profile.job_workers());
}

#[test]
fn web_profile_gives_inline_pools() {
    let profile = hand_profile(DeviceClass::Web);
    let app = App::new(TestGame, &(), &config(1, None), &profile).expect("app builds");
    assert_eq!(app.jobs().workers(), 0);
    let mut task = app.background().spawn(|| 7u32);
    // Inline pool: the job ran before `spawn` returned, so the result is already there.
    assert_eq!(task.try_take().expect("task not lost"), Some(7));
}

/// A game whose only system fails on its first tick.
struct FailingGame;

fn failing_system(_: &mut World, _: &mut SystemContext<'_>) -> Result<(), WorldError> {
    Err(WorldError::Invariant("test failure"))
}

impl Game for FailingGame {
    fn dynamic_entity_budget(&self) -> usize {
        0
    }

    type Command = ();
    type Settings = ();

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "failing-game",
            protocol_id: 0x0fa1,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        _: &mut World,
        schedule: &mut Schedule,
        _: &Self::Settings,
    ) -> Result<(), AppError> {
        schedule.add_system("failing", failing_system)?;
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        _: app::PlayerId,
        _: &engine_sessions::PlayerName,
    ) -> Result<NetId, AppError> {
        Ok(world.spawn()?)
    }

    fn leave(&self, _: &mut World, _: app::PlayerId) -> Result<(), AppError> {
        Ok(())
    }

    fn command(&self, _: &mut World, _: app::PlayerId, _: &()) -> Result<(), AppError> {
        Ok(())
    }
}

#[test]
fn failing_system_returns_world_error_and_tick_does_not_advance() {
    let profile = hand_profile(DeviceClass::Desktop);
    let mut app = App::new(FailingGame, &(), &config(1, Some(0)), &profile).expect("app builds");
    let result = app.step();
    assert!(matches!(result, Err(AppError::World(_))));
    assert_eq!(app.tick().raw(), 0);
}

#[test]
fn advance_past_the_cap_runs_the_cap_and_reports_the_dropped_ticks() {
    let profile = hand_profile(DeviceClass::Desktop);
    let cfg = config(2, Some(0));
    let mut app = App::new(TestGame, &(), &cfg, &profile).expect("app builds");
    let step = cfg.tick_rate.step_nanos();
    // Owes 20 ticks; the cap is `max_catch_up_ticks` = 8.
    let advance = app.advance(20 * step).expect("advance");
    assert_eq!(advance.ticks, 8);
    assert_eq!(advance.dropped_ticks, 12);
    assert_eq!(app.tick().raw(), 8);
    assert_eq!(counter_value(&app), 8);
    assert!(advance.remainder_nanos < step);
}
