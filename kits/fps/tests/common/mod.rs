//! Scene builders shared by the kit's integration tests. Each test binary includes this module with
//! `mod common;`, so the builders run exactly the systems a game schedules: bots, locomotion,
//! weapons, damage and the physics step, in that order.
//!
//! Every actor is a kinematic capsule of half height [`HALF_HEIGHT`] and radius [`RADIUS`], driven
//! through the kit's [`Locomotion`] component. The physics world has zero gravity; the character
//! controller applies its own gravity, as it does in the sandbox.

use std::error::Error;
use std::f32::consts::PI;

use app::ViewAngles;
use engine_core::{SimRng, Tick};
use engine_physics::{
    BodyBuilder, BodyId, ColliderBuilder, ColliderId, ColliderShape, ConvexShape, PhysicsConfig,
    PhysicsWorld, TriMeshShape,
};
use engine_world::{NetId, Schedule, SystemContext, World, WorldError};
use glam::{Vec2, Vec3};
use kit_fps::{
    Bot, BotConfig, BotParams, ControllerConfig, ControllerTuning, Health, LinkParams, LinkRules,
    Locomotion, Motor, MoveCommand, PhysicsSenses, Team, Trigger, Waypoints, Weapon, WeaponConfig,
    WeaponParams,
};

/// Fixed timestep: 60 ticks per second.
pub const DT: f32 = 1.0 / 60.0;
/// Half height of the capsule's cylinder, in metres.
pub const HALF_HEIGHT: f32 = 0.6;
/// Capsule radius, in metres.
pub const RADIUS: f32 = 0.3;
/// Eye height above the capsule centre, in metres.
pub const EYE_ABOVE_CENTRE: f32 = 0.7;
/// Capsule centre height when standing on the floor at `y = 0`.
pub const STANDING_CENTRE_Y: f32 = HALF_HEIGHT + RADIUS;

/// The test tuning. Jump speed 10 m/s gives a full jump of 2.5 m under 20 m/s^2 gravity, which is
/// high enough to clear a 1.5 m box with room for the controller's discretisation error.
pub fn tuning() -> ControllerTuning {
    ControllerTuning {
        walk_speed: 6.0,
        ground_accel: 60.0,
        ground_friction: 40.0,
        air_accel: 10.0,
        air_speed_cap: 7.0,
        jump_speed: 10.0,
        gravity: 20.0,
        max_fall_speed: 30.0,
        coyote_ticks: 3,
        jump_buffer_ticks: 4,
        jump_cut: 0.5,
        step_height: 0.4,
        slope_limit: 0.8,
        sprint_speed: 8.5,
        crouch_speed: 2.5,
        stand_height: 1.8,
        crouch_height: 1.1,
        slide_min_speed: 5.0,
        slide_boost: 1.5,
        slide_friction: 5.0,
        slide_ticks: 42,
        mantle_reach: 0.8,
        mantle_min_height: 0.2,
        mantle_max_height: 1.6,
        mantle_ticks: 42,
        camera_crouch_ticks: 10,
    }
}

/// The motor every scene uses.
pub fn motor() -> Result<Motor, Box<dyn Error>> {
    Ok(Motor::new(ControllerConfig::new(tuning())?, RADIUS)?)
}

/// An empty physics world with zero gravity.
pub fn physics() -> Result<PhysicsWorld, Box<dyn Error>> {
    Ok(PhysicsWorld::new(PhysicsConfig::new(DT, Vec3::ZERO)?))
}

/// Adds a static box at `centre` with the given half extents.
pub fn add_box(
    physics: &mut PhysicsWorld,
    centre: Vec3,
    half: Vec3,
) -> Result<ColliderId, Box<dyn Error>> {
    let builder = ColliderBuilder::new(ConvexShape::cuboid(half)?).translation(centre)?;
    Ok(physics.insert_collider(None, builder)?)
}

/// Adds a static box surface whose six sides are triangles.
pub fn add_triangle_mesh_box(
    physics: &mut PhysicsWorld,
    centre: Vec3,
    half: Vec3,
) -> Result<ColliderId, Box<dyn Error>> {
    let x0 = -half.x;
    let x1 = half.x;
    let y0 = -half.y;
    let y1 = half.y;
    let z0 = -half.z;
    let z1 = half.z;
    let quads = [
        [
            Vec3::new(x0, y1, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z1),
            Vec3::new(x1, y1, z1),
        ],
        [
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
        ],
        [
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z1),
        ],
        [
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
        ],
        [
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y1, z0),
        ],
        [
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
        ],
    ];
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut indices = Vec::with_capacity(quads.len() * 2);
    for [a, b, c, d] in quads {
        let offset = u32::try_from(vertices.len())?;
        vertices.extend([a, b, c, d]);
        indices.push([
            offset,
            offset.checked_add(2).ok_or("mesh index overflow")?,
            offset.checked_add(1).ok_or("mesh index overflow")?,
        ]);
        indices.push([
            offset.checked_add(1).ok_or("mesh index overflow")?,
            offset.checked_add(2).ok_or("mesh index overflow")?,
            offset.checked_add(3).ok_or("mesh index overflow")?,
        ]);
    }
    let shape = TriMeshShape::new(vertices, indices)?;
    let builder = ColliderBuilder::new(ColliderShape::TriMesh(shape)).translation(centre)?;
    Ok(physics.insert_collider(None, builder)?)
}

/// A world with the kit installed, the physics world and the motor inserted as resources.
pub fn world(physics: PhysicsWorld, seed: u64) -> Result<World, Box<dyn Error>> {
    let mut world = World::new(seed)?;
    kit_fps::install(&mut world)?;
    world.insert_resource(physics)?;
    world.insert_resource(motor()?)?;
    Ok(world)
}

/// The physics step as a system, so the schedule can order it after locomotion.
fn step_physics(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    world.resource_mut::<PhysicsWorld>()?.step();
    Ok(())
}

/// The kit's systems followed by the physics step. The seed forks each system's RNG stream.
pub fn schedule(seed: u64) -> Result<Schedule, Box<dyn Error>> {
    let mut schedule = Schedule::new(&SimRng::new(seed, 0), DT)?;
    kit_fps::add_systems(&mut schedule)?;
    schedule.add_system("physics.step", step_physics)?;
    Ok(schedule)
}

/// Runs one tick of the schedule.
pub fn run(world: &mut World, schedule: &mut Schedule, tick: u32) -> Result<(), Box<dyn Error>> {
    schedule.run_tick(world, Tick::new(tick))?;
    Ok(())
}

/// Spawns a standing capsule actor at `centre` on `team` with `health` hit points.
pub fn spawn_actor(
    world: &mut World,
    centre: Vec3,
    team: u8,
    health: u32,
) -> Result<(NetId, BodyId), Box<dyn Error>> {
    let (body, collider) = {
        let physics = world.resource_mut::<PhysicsWorld>()?;
        let body = physics.insert_body(BodyBuilder::kinematic().translation(centre)?);
        let capsule = ColliderBuilder::new(ConvexShape::capsule(HALF_HEIGHT, RADIUS)?);
        let collider = physics.insert_collider(Some(body), capsule)?;
        (body, collider)
    };
    let id = world.spawn()?;
    world.insert(
        id,
        Locomotion::new(body, collider, centre, EYE_ABOVE_CENTRE)?,
    )?;
    world.insert(id, Team::new(team))?;
    world.insert(id, Health::full(health)?)?;
    Ok((id, body))
}

/// Sets the command an actor is driven by for the next tick.
pub fn set_command(
    world: &mut World,
    id: NetId,
    command: MoveCommand,
) -> Result<(), Box<dyn Error>> {
    world.get_mut::<Locomotion>(id)?.set_command(command);
    Ok(())
}

/// A command with the given move axis and jump buttons, facing yaw zero.
pub fn walk(axis: Vec2, pressed: bool, held: bool) -> Result<MoveCommand, Box<dyn Error>> {
    Ok(MoveCommand::new(
        axis,
        ViewAngles::ZERO,
        pressed,
        held,
        Trigger::Released,
        false,
    )?)
}

/// A standing command with no movement or buttons, facing `yaw`.
pub fn facing(yaw: f32) -> Result<MoveCommand, Box<dyn Error>> {
    Ok(MoveCommand::new(
        Vec2::ZERO,
        ViewAngles::new(yaw, 0.0)?,
        false,
        false,
        Trigger::Released,
        false,
    )?)
}

/// A command that fires (or not) while facing yaw zero.
pub fn aim_and_fire(fire: bool) -> Result<MoveCommand, Box<dyn Error>> {
    Ok(MoveCommand::new(
        Vec2::ZERO,
        ViewAngles::ZERO,
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

/// The capsule centre of an actor.
pub fn centre_of(world: &World, id: NetId) -> Result<Vec3, Box<dyn Error>> {
    Ok(world.get::<Locomotion>(id)?.centre())
}

/// Whether an actor stood on the ground at the end of the last tick.
pub fn grounded(world: &World, id: NetId) -> Result<bool, Box<dyn Error>> {
    Ok(world.get::<Locomotion>(id)?.grounded())
}

/// Current hit points of an actor.
pub fn health_of(world: &World, id: NetId) -> Result<u32, Box<dyn Error>> {
    Ok(world.get::<Health>(id)?.current())
}

/// A rifle: one pellet of 25 damage every 6 ticks, 30 rounds, 90 tick reload, 60 m range.
pub fn rifle() -> Result<WeaponConfig, Box<dyn Error>> {
    Ok(WeaponConfig::new(WeaponParams {
        damage: 25,
        fire_interval_ticks: 6,
        magazine: 30,
        reload_ticks: 90,
        spread_rad: 0.02,
        spread_per_shot: 0.005,
        spread_recovery_per_tick: 0.002,
        max_range: 60.0,
        damage_falloff: kit_fps::DamageFalloff {
            full_until_m: 20.0,
            min_from_m: 40.0,
            min_scale: 0.5,
        },
        pellets: 1,
        friendly_fire: false,
    })?)
}

/// Bot tuning used by the scenes, with the given forget window in ticks. Quick to notice, and a
/// small aim error.
pub fn bot_params(forget_ticks: u32) -> BotParams {
    BotParams {
        sight_range: 30.0,
        fov_rad: 2.0,
        reaction_ticks: 20,
        aim_speed_rad_per_s: 4.0,
        aim_error_rad: 0.02,
        engage_range: 25.0,
        stop_distance: 1.0,
        strafe_period_ticks: 60,
        jump_chance: 0.0,
        turn_speed: 3.0,
        forget_ticks,
        arrive_radius: 1.5,
        stuck_ticks: 90,
        stuck_progress: 0.3,
    }
}

/// The duel scenes' bot: forgets its enemy after two seconds without sight.
pub fn bot_config() -> Result<BotConfig, Box<dyn Error>> {
    Ok(BotConfig::new(bot_params(120))?)
}

/// Link rules the scenes use: sight lines at knee and head height across a capsule's width, the
/// controller's step height, and links up to 25 metres.
pub fn link_rules() -> Result<LinkRules, Box<dyn Error>> {
    Ok(LinkRules::new(LinkParams {
        clearance_radius: RADIUS,
        knee_height: 0.5,
        head_height: 1.5,
        max_step: tuning().step_height,
        max_link: 25.0,
    })?)
}

/// Builds the waypoint graph for the static geometry in `physics`, before any actor is spawned.
pub fn waypoints(physics: &PhysicsWorld, points: Vec<Vec3>) -> Result<Waypoints, Box<dyn Error>> {
    let senses = PhysicsSenses::new(physics);
    Ok(Waypoints::build(points, &senses, &link_rules()?)?)
}

/// A duel scene: a floor, an optional wall across the line between the two actors, a stationary
/// player at the origin (team 0) and an actor ten metres down -Z (team 1, the bot's position).
pub struct Duel {
    /// The world holding both actors.
    pub world: World,
    /// The systems that run each tick.
    pub schedule: Schedule,
    /// The player, at the origin.
    pub player: NetId,
    /// The actor ten metres ahead, which the bot controls in the bot scenes.
    pub bot: NetId,
}

/// Builds a [`Duel`]. The wall, when present, stands five metres ahead, 3 m tall and 10 m wide.
pub fn duel(seed: u64, wall: bool) -> Result<Duel, Box<dyn Error>> {
    let mut physics = physics()?;
    add_box(
        &mut physics,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(50.0, 0.5, 50.0),
    )?;
    if wall {
        add_box(
            &mut physics,
            Vec3::new(0.0, 1.5, -5.0),
            Vec3::new(5.0, 1.5, 0.5),
        )?;
    }
    let nav = waypoints(
        &physics,
        vec![Vec3::new(-3.0, 0.0, -10.0), Vec3::new(3.0, 0.0, -10.0)],
    )?;
    let mut world = world(physics, seed)?;
    let (player, _) = spawn_actor(&mut world, Vec3::new(0.0, STANDING_CENTRE_Y, 0.0), 0, 100)?;
    let (bot, _) = spawn_actor(&mut world, Vec3::new(0.0, STANDING_CENTRE_Y, -10.0), 1, 100)?;
    world.insert(player, Weapon::new(rifle()?))?;
    world.insert(bot, Weapon::new(rifle()?))?;
    world.insert(bot, Bot::new(bot_config()?, SimRng::new(seed, 1)))?;
    // Face the bot towards the player (+Z is yaw pi), so the player is inside its field of view.
    set_command(&mut world, bot, facing(PI)?)?;
    world.insert_resource(nav)?;
    let schedule = schedule(seed)?;
    Ok(Duel {
        world,
        schedule,
        player,
        bot,
    })
}
