//! Shared fixtures for the client tests: a game with one body that moves one metre per tick,
//! and the settings they all use. Each test binary uses every item here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::num::NonZeroUsize;

use app::{AppConfig, AppError, Game, GameInfo, TickRate};
use engine_world::{Schedule, SystemContext, Transform, World, WorldError};
use glam::Vec3;
use platform::{CpuProfile, DeviceClass, DeviceProfile};

/// Moves every body one metre along +X on each tick.
pub struct Mover;

fn step_right(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let ids: Vec<_> = world.net_ids().collect();
    for id in ids {
        let mut transform = world.get_mut::<Transform>(id)?;
        let next = transform.translation() + Vec3::X;
        transform.set_translation(next)?;
    }
    Ok(())
}

impl Game for Mover {
    type Command = ();
    type Settings = ();

    fn dynamic_entity_budget(&self) -> usize {
        3
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "mover",
            protocol_id: 0x6d6f_7665,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        _: &Self::Settings,
    ) -> Result<(), AppError> {
        world.spawn()?;
        schedule.add_system("step_right", step_right)?;
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        _: app::PlayerId,
        _: &engine_sessions::PlayerName,
    ) -> Result<engine_world::NetId, AppError> {
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

/// A 60 Hz config with no worker threads.
pub fn config() -> AppConfig {
    AppConfig {
        tick_rate: TickRate::new(60).expect("valid rate"),
        seed: 7,
        max_catch_up_ticks: 8,
        job_workers: Some(0),
    }
}

/// A desktop-class profile with four cores.
pub fn profile() -> DeviceProfile {
    DeviceProfile {
        class: DeviceClass::Desktop,
        touch: false,
        cpu: CpuProfile {
            logical_cores: NonZeroUsize::new(4).expect("non-zero"),
            performance_cores: NonZeroUsize::new(4).expect("non-zero"),
        },
        memory_bytes: None,
        gpu: None,
    }
}
