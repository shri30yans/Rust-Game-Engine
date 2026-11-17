//! Helpers shared by the arena's integration tests. Each test binary includes this module with
//! `mod common;`, so every helper is not used by every binary; the module allows dead code for that.
//!
//! Two harnesses: [`Arena`] runs the game's own schedule (bots, locomotion, physics, weapons,
//! feedback, damage, rules, transform sync), and [`Motion`] runs only the kit's locomotion and the
//! physics step against the arena's boxes, for movement tests that must not depend on bots.
//! Aim helpers use the engine's convention: yaw 0 looks down -Z, and right is +X.

#![allow(dead_code)]

use std::error::Error;

use app::{GameEntry, ViewAngles};
use engine_assets::{ContentPack, PackReader};
use engine_core::{SimRng, Tick};
use engine_physics::{
    BodyBuilder, ColliderBuilder, ConvexShape, PhysicsConfig, PhysicsWorld, QueryFilter, RayCast,
};
use engine_world::{NetId, Schedule, SystemContext, SystemFn, World, WorldError};
use glam::{Vec2, Vec3};
use imagine::data::{
    CAPSULE_RADIUS_M, CENTRE_ABOVE_FEET_M, EYE_ABOVE_CENTRE_M, TICK_RATE_HZ, load_tuning,
};
use imagine::sim::actor_id;
use imagine::sim::{ImagineSettings, add_arena_systems, setup_arena_with_settings};
use kit_fps::{Health, Locomotion, Motor, MoveCommand, Trigger};

/// Result type of a test body: any error from the engine, the kit or the game fails the test.
pub type TestResult = Result<(), Box<dyn Error>>;

/// Opens the committed game pack through the same platform stream and reader as headless startup.
pub fn content_pack() -> Result<ContentPack, Box<dyn Error>> {
    let stream = platform::ContentSource::headless()
        .open_content(imagine::ImagineEntry::CONTENT.as_str())?;
    Ok(PackReader::read(stream)?)
}

/// Seconds per tick, from the game's tick rate.
pub fn dt() -> f32 {
    1.0 / f32::from(TICK_RATE_HZ)
}

/// Number of actors configured for a world, including the player.
pub fn actor_count(world: &World) -> Result<usize, engine_world::WorldError> {
    Ok(world.resource::<imagine::rules::Actors>()?.slots().len())
}

/// Height of an actor's eyes above the floor, in metres.
pub const EYE_HEIGHT_M: f32 = CENTRE_ABOVE_FEET_M + EYE_ABOVE_CENTRE_M;

/// The full game: every actor, the scene and the schedule, built by the game's own setup.
pub struct Arena {
    /// The world the schedule runs on.
    pub world: World,
    schedule: Schedule,
    tick: u32,
}

impl Arena {
    /// The arena with `seed`, ready at tick zero.
    ///
    /// # Errors
    ///
    /// Returns any error from building the world, the schedule or the arena's setup.
    pub fn new(seed: u64) -> Result<Self, Box<dyn Error>> {
        Self::with_settings(seed, &ImagineSettings::default())
    }

    /// The arena with explicit match and arena data settings, ready at tick zero.
    pub fn with_settings(seed: u64, settings: &ImagineSettings) -> Result<Self, Box<dyn Error>> {
        let mut world = World::new(seed)?;
        let mut schedule = Schedule::new(&SimRng::new(seed, 0), dt())?;
        setup_arena_with_settings(&mut world, &mut schedule, &content_pack()?, settings)?;
        Ok(Self {
            world,
            schedule,
            tick: 0,
        })
    }

    /// The default arena with `seed`, scheduled with the event readers in the order given.
    ///
    /// # Errors
    ///
    /// Returns any error from building the world, the schedule or the arena's setup.
    pub fn with_readers(
        seed: u64,
        readers: &[(&'static str, SystemFn)],
    ) -> Result<Self, Box<dyn Error>> {
        let mut world = World::new(seed)?;
        // The game's setup builds the world and its own schedule; this test schedule replaces it.
        let mut setup_schedule = Schedule::new(&SimRng::new(seed, 0), dt())?;
        setup_arena_with_settings(
            &mut world,
            &mut setup_schedule,
            &content_pack()?,
            &ImagineSettings::default(),
        )?;
        let mut schedule = Schedule::new(&SimRng::new(seed, 0), dt())?;
        add_arena_systems(&mut schedule, readers)?;
        Ok(Self {
            world,
            schedule,
            tick: 0,
        })
    }

    /// Runs one tick.
    ///
    /// # Errors
    ///
    /// Returns the first error any system reports.
    pub fn step(&mut self) -> TestResult {
        self.schedule
            .run_tick(&mut self.world, Tick::new(self.tick))?;
        self.tick = self.tick.saturating_add(1);
        Ok(())
    }

    /// Runs `ticks` ticks.
    ///
    /// # Errors
    ///
    /// Returns the first error any system reports.
    pub fn run(&mut self, ticks: u32) -> TestResult {
        for _ in 0..ticks {
            self.step()?;
        }
        Ok(())
    }

    /// The tick the next call to [`Arena::step`] simulates.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Sets the player's command for the next tick.
    ///
    /// # Errors
    ///
    /// Returns a world error when the player is missing.
    pub fn command_player(&mut self, command: MoveCommand) -> TestResult {
        imagine::sim::set_player_command(&mut self.world, command)?;
        Ok(())
    }
}

/// A command with the given forward input (`+1` walks forward), aim and fire button.
///
/// # Errors
///
/// Returns an error when the aim is not finite.
pub fn command(
    forward: f32,
    yaw: f32,
    pitch: f32,
    fire: bool,
) -> Result<MoveCommand, Box<dyn Error>> {
    Ok(MoveCommand::new(
        Vec2::new(0.0, forward),
        ViewAngles::new(yaw, pitch)?,
        false,
        false,
        if fire {
            Trigger::Held
        } else {
            Trigger::Released
        },
        false,
    )?)
}

/// The yaw that looks from `from` towards `to` on the XZ plane. Yaw 0 looks down -Z.
pub fn aim_yaw(from: Vec3, to: Vec3) -> f32 {
    let delta = to - from;
    (-delta.x).atan2(-delta.z)
}

/// The pitch that looks from `from` towards `to`: positive looks up.
pub fn aim_pitch(from: Vec3, to: Vec3) -> f32 {
    let delta = to - from;
    delta.y.atan2(Vec2::new(delta.x, delta.z).length())
}

/// The centre of the actor in `slot`.
///
/// # Errors
///
/// Returns a world error when the actor or its locomotion is missing.
pub fn centre_of(world: &World, slot: usize) -> Result<Vec3, Box<dyn Error>> {
    let id = actor_id(world, slot)?;
    Ok(world.get::<Locomotion>(id)?.centre())
}

/// The eye point of the actor in `slot`.
///
/// # Errors
///
/// Returns a world error when the actor or its locomotion is missing.
pub fn eye_of(world: &World, slot: usize) -> Result<Vec3, Box<dyn Error>> {
    Ok(centre_of(world, slot)? + Vec3::Y * EYE_ABOVE_CENTRE_M)
}

/// The current health of the actor in `slot`, as `(current, max)`.
///
/// # Errors
///
/// Returns a world error when the actor or its health is missing.
pub fn health_of(world: &World, slot: usize) -> Result<(u32, u32), Box<dyn Error>> {
    let id = actor_id(world, slot)?;
    let health = world.get::<Health>(id)?;
    Ok((health.current(), health.max()))
}

/// Whether the straight segment from `from` to `to` meets no solid of the physics world before
/// it reaches `to`. Only static geometry is tested: use this where no actor stands on the segment.
///
/// # Errors
///
/// Returns a physics error when the segment is degenerate or not finite.
pub fn line_is_clear(physics: &PhysicsWorld, from: Vec3, to: Vec3) -> Result<bool, Box<dyn Error>> {
    let delta = to - from;
    let length = delta.length();
    if length <= 0.0 {
        return Ok(true);
    }
    let request = RayCast::new(from, delta / length, length, true)?;
    Ok(physics.ray_cast(request, QueryFilter::default()).is_none())
}

/// Advances the physics world by one fixed step. A system, so it can be scheduled.
fn step_physics(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    world.resource_mut::<PhysicsWorld>()?.step();
    Ok(())
}

/// The arena's geometry and one walking actor, with the kit's locomotion and the physics step and
/// nothing else. For movement tests.
pub struct Motion {
    /// The world holding the scene and the walker.
    pub world: World,
    schedule: Schedule,
    /// The walker's entity.
    pub id: NetId,
    tick: u32,
}

impl Motion {
    /// A walker standing with its feet at `feet` on the arena's geometry, with the game's movement
    /// tuning.
    ///
    /// # Errors
    ///
    /// Returns any error from building the physics world, the scene or the walker.
    pub fn new(feet: Vec3) -> Result<Self, Box<dyn Error>> {
        let mut world = World::new(1)?;
        kit_fps::install(&mut world, 1)?;
        world.insert_resource(PhysicsWorld::new(PhysicsConfig::new(dt(), Vec3::ZERO)?))?;
        let content = content_pack()?;
        let tuning = load_tuning(&content)?;
        world.insert_resource(tuning.clone())?;
        let motor =
            kit_fps::error::lift(Motor::new(tuning.controller_config()?, CAPSULE_RADIUS_M))?;
        world.insert_resource(motor)?;
        engine_scene::register_components(&mut world)?;
        world.insert_resource(imagine::scene::SceneContent(content))?;
        imagine::scene::start_scene_from_content(&mut world, 0)?;

        let centre = feet + Vec3::Y * CENTRE_ABOVE_FEET_M;
        let (body, collider) = {
            let physics = world.resource_mut::<PhysicsWorld>()?;
            let builder = BodyBuilder::kinematic().translation(centre)?;
            let body = physics.insert_body(builder);
            let shape = ConvexShape::capsule(
                tuning.movement.stand_height * 0.5 - CAPSULE_RADIUS_M,
                CAPSULE_RADIUS_M,
            )?;
            let collider = physics.insert_collider(Some(body), ColliderBuilder::new(shape))?;
            (body, collider)
        };
        let id = world.spawn()?;
        let locomotion =
            kit_fps::error::lift(Locomotion::new(body, collider, centre, EYE_ABOVE_CENTRE_M))?;
        world.insert(id, locomotion)?;

        let mut schedule = Schedule::new(&SimRng::new(1, 0), dt())?;
        schedule.add_system("fps.locomotion", kit_fps::locomotion_system)?;
        schedule.add_system("physics.step", step_physics)?;
        Ok(Self {
            world,
            schedule,
            id,
            tick: 0,
        })
    }

    /// Sets the walker's command and runs one tick.
    ///
    /// # Errors
    ///
    /// Returns the first error any system reports.
    pub fn step(&mut self, command: MoveCommand) -> TestResult {
        self.world
            .get_mut::<Locomotion>(self.id)?
            .set_command(command);
        self.schedule
            .run_tick(&mut self.world, Tick::new(self.tick))?;
        self.tick = self.tick.saturating_add(1);
        Ok(())
    }

    /// The walker's centre.
    ///
    /// # Errors
    ///
    /// Returns a world error when the walker's locomotion is missing.
    pub fn centre(&self) -> Result<Vec3, Box<dyn Error>> {
        Ok(self.world.get::<Locomotion>(self.id)?.centre())
    }

    /// Whether the walker is standing on something.
    ///
    /// # Errors
    ///
    /// Returns a world error when the walker's locomotion is missing.
    pub fn grounded(&self) -> Result<bool, Box<dyn Error>> {
        Ok(self.world.get::<Locomotion>(self.id)?.grounded())
    }

    /// The height of the walker's feet above the floor, in metres.
    ///
    /// # Errors
    ///
    /// Returns a world error when the walker's locomotion is missing.
    pub fn feet_height(&self) -> Result<f32, Box<dyn Error>> {
        Ok(self.centre()?.y - CENTRE_ABOVE_FEET_M)
    }
}

/// A walk command: `forward` on the Y axis while facing `yaw`. `jump_pressed` is true only on the
/// tick the button goes down; `jump_held` is the button's level and must stay set while rising.
///
/// # Errors
///
/// Returns an error when the aim is not finite.
pub fn walk(
    yaw: f32,
    forward: f32,
    jump_pressed: bool,
    jump_held: bool,
) -> Result<MoveCommand, Box<dyn Error>> {
    Ok(MoveCommand::new(
        Vec2::new(0.0, forward),
        ViewAngles::new(yaw, 0.0)?,
        jump_pressed,
        jump_held,
        Trigger::Released,
        false,
    )?)
}
