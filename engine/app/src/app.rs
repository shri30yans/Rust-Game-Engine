//! The app: owns the world, the schedule, the clock, the tick counter and the
//! two job pools, and runs fixed ticks.
//!
//! Invariants: the tick counter never wraps (stepping at `u32::MAX` is an
//! error, checked before any system runs); the pools belong to the app and are
//! built from the device profile unless the config overrides the worker count;
//! nothing here reads the wall clock.

use engine_core::{SimRng, Tick};
use engine_scene::{Level, SceneError};
use engine_world::{Schedule, World};
use jobs::{BackgroundPool, JobPool};
use platform::{DeviceClass, DeviceProfile};

use crate::clock::{Advance, FixedClock};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::game::Game;
use crate::game::PlayerId;
use engine_sessions::PlayerName;
use engine_world::NetId;

/// A game running inside the engine's fixed-tick loop.
pub struct App<G: Game> {
    game: G,
    world: World,
    schedule: Schedule,
    tick: Tick,
    travel_epoch: u32,
    seed: u64,
    dt_seconds: f32,
    clock: FixedClock,
    jobs: JobPool,
    background: BackgroundPool,
}

impl<G: Game> App<G> {
    /// Builds the first world from `settings` and `config.seed`, then builds the app pools.
    ///
    /// The job pool gets `config.job_workers`, or the profile's count when that is `None`. The
    /// background pool gets 0 workers on the web (jobs run inline) and 1 worker elsewhere.
    ///
    /// # Errors
    ///
    /// Returns an error for a bad clock setting, a failed world setup or game setup, or a pool
    /// that cannot be built.
    pub fn new(
        game: G,
        settings: &G::Settings,
        config: &AppConfig,
        profile: &DeviceProfile,
    ) -> Result<Self, AppError> {
        let clock = FixedClock::new(config.tick_rate, config.max_catch_up_ticks)?;
        let mut world = World::new(config.seed)?;
        let mut schedule = Schedule::new(world.rng(), config.tick_rate.dt_seconds())?;
        game.setup(&mut world, &mut schedule, settings)?;
        let workers = config.job_workers.unwrap_or_else(|| profile.job_workers());
        let jobs = JobPool::new(workers)?;
        let background_workers = match profile.class {
            DeviceClass::Web => 0,
            DeviceClass::Desktop | DeviceClass::Handheld | DeviceClass::Server => 1,
        };
        let background = BackgroundPool::new(background_workers)?;
        Ok(Self {
            game,
            world,
            schedule,
            tick: Tick::ZERO,
            travel_epoch: 0,
            seed: config.seed,
            dt_seconds: config.tick_rate.dt_seconds(),
            clock,
            jobs,
            background,
        })
    }

    /// The game this app runs.
    #[must_use]
    pub const fn game(&self) -> &G {
        &self.game
    }

    /// Shared access to the world.
    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// Exclusive access to the world, for tests and for scene setup before the first tick.
    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Invokes the game's join hook for a player.
    pub(crate) fn join_player(
        &mut self,
        player: PlayerId,
        name: &PlayerName,
    ) -> Result<NetId, AppError> {
        self.game.join(&mut self.world, player, name)
    }

    /// Invokes the game's leave hook for a player.
    pub(crate) fn leave_player(&mut self, player: PlayerId) -> Result<(), AppError> {
        self.game.leave(&mut self.world, player)
    }

    /// Applies a player's command through the game's single command path.
    pub(crate) fn apply_player_command(
        &mut self,
        player: PlayerId,
        command: &G::Command,
    ) -> Result<(), AppError> {
        self.game.command(&mut self.world, player, command)
    }

    pub(crate) fn scene_reloaded(
        &mut self,
        level: &Level,
        content: &engine_assets::ContentPack,
    ) -> Result<(), SceneError> {
        self.game.scene_reloaded(&mut self.world, level, content)
    }

    /// Number of ticks run so far.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The world generation, incremented only after a successful travel.
    #[must_use]
    pub const fn travel_epoch(&self) -> u32 {
        self.travel_epoch
    }

    /// Builds a replacement world and schedule, then swaps them in only if setup succeeds.
    /// The tick counter and app pools remain alive across travel.
    pub fn travel(&mut self, settings: &G::Settings) -> Result<(), AppError> {
        let epoch = self
            .travel_epoch
            .checked_add(1)
            .ok_or(AppError::TravelEpochOverflow)?;
        let seed = SimRng::new(self.seed, u64::from(epoch)).next_u64();
        let mut world = World::new(seed)?;
        let mut schedule = Schedule::new(world.rng(), self.dt_seconds)?;
        self.game.setup(&mut world, &mut schedule, settings)?;
        self.world = world;
        self.schedule = schedule;
        self.travel_epoch = epoch;
        Ok(())
    }

    /// The fork-join pool for chunked work inside a step.
    #[must_use]
    pub const fn jobs(&self) -> &JobPool {
        &self.jobs
    }

    /// The background pool for work that no frame waits on.
    #[must_use]
    pub const fn background(&self) -> &BackgroundPool {
        &self.background
    }

    /// Runs exactly one tick: every system in order, then the tick counter advances.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::TickOverflow`] when the counter is at `u32::MAX`; no system runs then.
    /// Returns [`AppError::World`] when a system fails. Systems before it have already run.
    pub fn step(&mut self) -> Result<(), AppError> {
        if self.tick == Tick::new(u32::MAX) {
            return Err(AppError::TickOverflow);
        }
        self.schedule.run_tick(&mut self.world, self.tick)?;
        self.tick = self.tick.next();
        Ok(())
    }

    /// Feeds one frame's elapsed nanoseconds to the clock and runs the ticks it asks for.
    ///
    /// # Errors
    ///
    /// Returns the first error from [`App::step`].
    pub fn advance(&mut self, frame_nanos: u64) -> Result<Advance, AppError> {
        self.advance_with(frame_nanos, |_, _| Ok(()))
    }

    /// Feeds frame time to the fixed clock and invokes `before_tick` before every due step.
    pub fn advance_with(
        &mut self,
        frame_nanos: u64,
        mut before_tick: impl FnMut(&G, &mut World) -> Result<(), AppError>,
    ) -> Result<Advance, AppError> {
        let advance = self.clock.advance(frame_nanos);
        for _ in 0..advance.ticks {
            before_tick(&self.game, &mut self.world)?;
            self.step()?;
        }
        Ok(advance)
    }

    /// Advances the fixed clock while an external client session owns simulation for each tick.
    pub fn advance_external(
        &mut self,
        frame_nanos: u64,
        mut before_tick: impl FnMut(&mut World, Tick) -> Result<(), AppError>,
    ) -> Result<Advance, AppError> {
        let advance = self.clock.advance(frame_nanos);
        for _ in 0..advance.ticks {
            if self.tick == Tick::new(u32::MAX) {
                return Err(AppError::TickOverflow);
            }
            before_tick(&mut self.world, self.tick)?;
            self.tick = self.tick.next();
        }
        Ok(advance)
    }

    /// State hash of the world now.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::World`] if a component fails to encode.
    pub fn state_hash(&self) -> Result<u64, AppError> {
        self.world.state_hash().map_err(AppError::World)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::clock::TickRate;
    use crate::game::GameInfo;
    use platform::CpuProfile;
    use std::num::NonZeroUsize;

    struct Empty;

    impl Game for Empty {
        fn dynamic_entity_budget(&self) -> usize {
            0
        }

        type Command = ();
        type Settings = ();

        fn info(&self) -> GameInfo {
            GameInfo {
                name: "empty",
                protocol_id: 0,
                protocol_version: 0,
            }
        }

        fn setup(
            &self,
            _: &mut World,
            _: &mut Schedule,
            _: &Self::Settings,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn join(
            &self,
            world: &mut World,
            _: crate::PlayerId,
            _: &engine_sessions::PlayerName,
        ) -> Result<engine_world::NetId, AppError> {
            Ok(world.spawn()?)
        }

        fn leave(&self, _: &mut World, _: crate::PlayerId) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &self,
            _: &mut World,
            _: crate::PlayerId,
            _: &Self::Command,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TravelSettings {
        fail: bool,
    }

    struct TravelGame;

    #[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    struct TravelValue(u32);

    impl Game for TravelGame {
        fn dynamic_entity_budget(&self) -> usize {
            0
        }

        type Command = ();
        type Settings = TravelSettings;

        fn info(&self) -> GameInfo {
            GameInfo {
                name: "travel-test",
                protocol_id: 2,
                protocol_version: 1,
            }
        }

        fn setup(
            &self,
            world: &mut World,
            _schedule: &mut Schedule,
            settings: &Self::Settings,
        ) -> Result<(), AppError> {
            world.register::<TravelValue>(engine_world::Replication::Replicated)?;
            let mut rng = world.rng().clone();
            let id = world.spawn()?;
            world.insert(id, TravelValue(rng.next_u32()))?;
            if settings.fail {
                return Err(AppError::World(engine_world::WorldError::Invariant(
                    "requested test setup failure",
                )));
            }
            Ok(())
        }

        fn join(
            &self,
            world: &mut World,
            _player: crate::PlayerId,
            _name: &engine_sessions::PlayerName,
        ) -> Result<engine_world::NetId, AppError> {
            world
                .net_ids()
                .next()
                .ok_or(engine_world::WorldError::Invariant("travel test pawn missing").into())
        }

        fn leave(&self, _world: &mut World, _player: crate::PlayerId) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &self,
            _world: &mut World,
            _player: crate::PlayerId,
            _command: &Self::Command,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn desktop_profile() -> DeviceProfile {
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

    #[test]
    fn step_at_max_tick_is_an_error_and_does_not_wrap() {
        let config = AppConfig {
            tick_rate: TickRate::new(60).expect("valid rate"),
            seed: 1,
            max_catch_up_ticks: 4,
            job_workers: Some(0),
        };
        let mut app = App::new(Empty, &(), &config, &desktop_profile()).expect("app builds");
        app.tick = Tick::new(u32::MAX);
        assert!(matches!(app.step(), Err(AppError::TickOverflow)));
        assert_eq!(app.tick(), Tick::new(u32::MAX));
    }

    #[test]
    fn travel_is_build_then_swap_and_keeps_the_tick_monotonic() {
        let config = AppConfig {
            tick_rate: TickRate::new(60).expect("valid rate"),
            seed: 17,
            max_catch_up_ticks: 4,
            job_workers: Some(0),
        };
        let settings = TravelSettings { fail: false };
        let mut app = App::new(TravelGame, &settings, &config, &desktop_profile())
            .expect("initial world builds");
        app.step().expect("initial tick");
        let old_hash = app.state_hash().expect("old world hash");
        assert_eq!(app.tick(), Tick::new(1));

        assert!(app.travel(&TravelSettings { fail: true }).is_err());
        assert_eq!(app.travel_epoch(), 0);
        assert_eq!(app.tick(), Tick::new(1));
        assert_eq!(app.state_hash().expect("old world remains"), old_hash);

        app.travel(&settings).expect("first travel");
        assert_eq!(app.travel_epoch(), 1);
        assert_eq!(app.tick(), Tick::new(1));
        app.travel(&settings).expect("second travel");
        assert_eq!(app.travel_epoch(), 2);
        assert_eq!(app.tick(), Tick::new(1));
    }

    #[test]
    fn equivalent_travels_have_identical_world_state() {
        let config = AppConfig {
            tick_rate: TickRate::new(60).expect("valid rate"),
            seed: 27,
            max_catch_up_ticks: 4,
            job_workers: Some(0),
        };
        let settings = TravelSettings { fail: false };
        let mut left =
            App::new(TravelGame, &settings, &config, &desktop_profile()).expect("left app");
        let mut right =
            App::new(TravelGame, &settings, &config, &desktop_profile()).expect("right app");
        left.travel(&settings).expect("left travel");
        right.travel(&settings).expect("right travel");
        assert_eq!(
            left.state_hash().expect("left hash"),
            right.state_hash().expect("right hash")
        );
    }
}
