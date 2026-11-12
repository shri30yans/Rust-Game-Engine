//! The sandbox simulation: the player's character, the physics step and the transform sync.
//!
//! Input enters only through [`set_command`], as a [`PlayerCommand`] whose parts are [`Axis`]
//! values and a validated [`ViewAngles`], so every command the simulation can receive is valid by
//! construction. State lives in
//! two resources, [`Player`] and the physics world; the three systems run in this order each tick:
//! `character_move`, `physics_step`, `sync_transforms`.
//!
//! Invariants: the character state changes only in `character_move` (and in [`place_player`],
//! a scenario helper); the transform sync allocates nothing per tick; the yaw stays in
//! `(-PI, PI]`; the tick rate is [`TICK_RATE_HZ`].

use std::f32::consts::{PI, TAU};

use app::{AppConfig, AppError, Game, GameInfo, PlayerId, TickRate, ViewAngles};
use engine_assets::ContentPack;
use engine_core::StateHasher;
use engine_physics::{
    BodyBuilder, BodyId, CharacterConfig, CharacterController, CharacterInput, CharacterState,
    ColliderBuilder, PhysicsConfig, PhysicsError, PhysicsWorld,
};
use engine_world::{NetId, Resource, Schedule, SystemContext, World, WorldError};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::scene::{
    Bodies, PLAYER_SPAWN_M, SceneBody, apply_layout, physics, start_level, write_pose,
};

/// Simulation rate, in ticks per second.
pub const TICK_RATE_HZ: u16 = 60;
/// Downward acceleration, in metres per second squared. Used by the world and the character.
pub const GRAVITY_MPS2: f32 = 9.81;
const SAMPLE_ARENA_PLAYER_SPAWN_M: Vec3 = Vec3::new(20.0, 5.0, -5.0);
/// Ground speed at full wish, in metres per second.
pub const WALK_SPEED_MPS: f32 = 4.0;
/// Launch speed of a jump, in metres per second.
pub const JUMP_SPEED_MPS: f32 = 5.0;
/// Terminal fall speed, in metres per second.
pub const MAX_FALL_SPEED_MPS: f32 = 50.0;
/// Half the height of the capsule's straight section, in metres.
pub const CAPSULE_HALF_HEIGHT_M: f32 = 0.5;
/// Radius of the capsule, in metres. The feet are `CENTRE_ABOVE_FEET_M` below the centre.
pub const CAPSULE_RADIUS_M: f32 = 0.3;
/// Distance from the capsule centre down to its lowest point, in metres.
pub const CENTRE_ABOVE_FEET_M: f32 = CAPSULE_HALF_HEIGHT_M + CAPSULE_RADIUS_M;
/// Eye height above the feet, in metres (the camera position, not a collider).
pub const EYE_HEIGHT_M: f32 = 1.5;
/// Gap the character keeps from geometry, in metres.
pub const SKIN_WIDTH_M: f32 = 0.01;
/// Tallest obstacle the character climbs without jumping, in metres.
pub const STEP_HEIGHT_M: f32 = 0.3;
/// Narrowest obstacle the character climbs, in metres.
pub const STEP_MIN_WIDTH_M: f32 = 0.2;
/// Steepest walkable slope, in degrees.
pub const SLOPE_LIMIT_DEG: f32 = 45.0;
/// Distance below the character that still counts as ground, in metres.
pub const GROUND_SNAP_M: f32 = 0.15;

/// One signed input axis: negative, neutral or positive. An enum rather than a float, so a
/// command cannot carry a value that is not one of these three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// Full input in the negative direction.
    Negative,
    /// No input.
    Zero,
    /// Full input in the positive direction.
    Positive,
}

impl Axis {
    /// The axis as a number: -1, 0 or 1.
    #[must_use]
    pub const fn value(self) -> f32 {
        match self {
            Self::Negative => -1.0,
            Self::Zero => 0.0,
            Self::Positive => 1.0,
        }
    }

    /// The axis from a positive and a negative press. Both or neither gives zero.
    #[must_use]
    pub const fn from_presses(positive: bool, negative: bool) -> Self {
        match (positive, negative) {
            (true, false) => Self::Positive,
            (false, true) => Self::Negative,
            _ => Self::Zero,
        }
    }
}

/// One tick of player input: two movement axes, a jump press and the view heading.
///
/// The view is a [`ViewAngles`], so it is finite and wrapped by construction. The simulation
/// takes the character's yaw from it; the engine's mouse look produces it (see the presenter).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawPlayerCommand", into = "RawPlayerCommand")]
pub struct PlayerCommand {
    strafe: Axis,
    forward: Axis,
    jump: bool,
    view: ViewAngles,
}

/// Untrusted wire representation of the sandbox command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RawPlayerCommand {
    strafe: Axis,
    forward: Axis,
    jump: bool,
    yaw: f32,
}

impl TryFrom<RawPlayerCommand> for PlayerCommand {
    type Error = AppError;

    fn try_from(raw: RawPlayerCommand) -> Result<Self, Self::Error> {
        Ok(Self::new(
            raw.strafe,
            raw.forward,
            raw.jump,
            ViewAngles::new(raw.yaw, 0.0)?,
        ))
    }
}

impl From<PlayerCommand> for RawPlayerCommand {
    fn from(command: PlayerCommand) -> Self {
        Self {
            strafe: command.strafe,
            forward: command.forward,
            jump: command.jump,
            yaw: command.view.yaw(),
        }
    }
}

impl engine_netcode::InputPayload for PlayerCommand {
    const MAX_BYTES: usize = 32;

    fn validate(&self) -> Result<(), engine_netcode::NetError> {
        Ok(())
    }
}

impl PlayerCommand {
    /// No movement, no jump, facing down -Z (yaw 0).
    pub const IDLE: Self = Self::new(Axis::Zero, Axis::Zero, false, ViewAngles::ZERO);

    /// A command from its parts. `strafe` is right(+), `forward` is ahead(+), `jump` starts a
    /// jump when the character is grounded, and `view` is the heading the character faces.
    #[must_use]
    pub const fn new(strafe: Axis, forward: Axis, jump: bool, view: ViewAngles) -> Self {
        Self {
            strafe,
            forward,
            jump,
            view,
        }
    }

    /// The horizontal wish, each axis in `[-1, 1]` and length at most 1 (diagonals are not faster).
    fn wish(self) -> Vec2 {
        Vec2::new(self.strafe.value(), self.forward.value()).clamp_length_max(1.0)
    }
}

/// The player's character: its controller, state, yaw and the entities that mirror it.
pub struct Player {
    controller: CharacterController,
    state: CharacterState,
    yaw: f32,
    id: NetId,
    body: BodyId,
    command: PlayerCommand,
}

impl Player {
    /// The character's state after the last tick.
    #[must_use]
    pub const fn state(&self) -> CharacterState {
        self.state
    }

    /// The view heading in radians, in `(-PI, PI]`. Yaw 0 looks down -Z.
    #[must_use]
    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The entity whose `Transform` mirrors the character.
    #[must_use]
    pub const fn id(&self) -> NetId {
        self.id
    }
}

impl Resource for Player {
    fn hash_into(&self, hasher: &mut StateHasher) {
        let position = self.state.position();
        hasher.write_f32(position.x);
        hasher.write_f32(position.y);
        hasher.write_f32(position.z);
        hasher.write_f32(self.state.vertical_velocity());
        hasher.write_u8(u8::from(self.state.grounded()));
        hasher.write_f32(self.yaw);
    }
}

/// The sandbox game: registers the physics world, the scene and the player, and adds the systems.
pub struct SandboxGame {
    content: ContentPack,
}

impl SandboxGame {
    /// Creates the sandbox game from its mounted content pack.
    #[must_use]
    pub fn from_content(content: &ContentPack) -> Self {
        Self {
            content: content.clone(),
        }
    }
}

impl Game for SandboxGame {
    type Command = PlayerCommand;
    type Settings = ();

    fn dynamic_entity_budget(&self) -> usize {
        0
    }

    fn scene_content(&self) -> Result<engine_scene::SceneContent, engine_scene::SceneError> {
        crate::scene::live_scene_content(&self.content)
    }

    fn scene_reloaded(
        &self,
        world: &mut World,
        level: &engine_scene::Level,
        content: &ContentPack,
    ) -> Result<(), engine_scene::SceneError> {
        world.insert_resource(crate::scene::SceneContent(content.clone()))?;
        crate::scene::refresh_live_scene(world, level)
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "sandbox",
            protocol_id: 1,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        _: &Self::Settings,
    ) -> Result<(), AppError> {
        let gravity = Vec3::new(0.0, -GRAVITY_MPS2, 0.0);
        let config = physics(PhysicsConfig::new(schedule.dt(), gravity), "physics config")?;
        world.insert_resource(PhysicsWorld::new(config))?;
        world.insert_resource(Bodies::default())?;
        engine_scene::register_components(world)?;
        world.insert_resource(crate::scene::SceneContent(self.content.clone()))?;
        start_level(world)?;
        let player = spawn_player(world, PLAYER_SPAWN_M)?;
        world.insert_resource(player)?;
        schedule.add_system("character_move", character_move)?;
        schedule.add_system("physics_step", physics_step)?;
        schedule.add_system("sync_transforms", sync_transforms)?;
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        _player: PlayerId,
        _name: &engine_sessions::PlayerName,
    ) -> Result<NetId, AppError> {
        Ok(world.resource::<Player>()?.id())
    }

    fn leave(&self, _world: &mut World, _player: PlayerId) -> Result<(), AppError> {
        Ok(())
    }

    fn command(
        &self,
        world: &mut World,
        _player: PlayerId,
        command: &Self::Command,
    ) -> Result<(), AppError> {
        set_command(world, *command)?;
        Ok(())
    }
}

/// The app configuration the sandbox runs with: 60 Hz, seed 1, eight catch-up ticks, room for
/// 256 entities. `job_workers` overrides the device's worker count when `Some`.
///
/// # Errors
///
/// Returns an error when the fixed tick rate is rejected (it cannot be, for [`TICK_RATE_HZ`]).
pub fn app_config(job_workers: Option<usize>) -> Result<AppConfig, AppError> {
    Ok(AppConfig {
        tick_rate: TickRate::new(TICK_RATE_HZ)?,
        seed: 1,
        max_catch_up_ticks: 8,
        job_workers,
    })
}

/// Records the command for the next tick. This is the only way input enters the simulation.
///
/// # Errors
///
/// Returns a world error when the player is missing.
pub fn set_command(world: &mut World, command: PlayerCommand) -> Result<(), WorldError> {
    world.resource_mut::<Player>()?.command = command;
    Ok(())
}

/// Moves the character to `position` with `yaw`, at rest and ungrounded. A scenario helper for
/// tests and demos; it is not input, so it does not run through [`set_command`].
///
/// # Errors
///
/// Returns a world error when the position is not finite or the player is missing.
pub fn place_player(world: &mut World, position: Vec3, yaw: f32) -> Result<(), WorldError> {
    let state = physics(CharacterState::new(position), "placed position")?;
    let rotation = Quat::from_rotation_y(yaw);
    let body = world.resource::<Player>()?.body;
    physics(
        world
            .resource_mut::<PhysicsWorld>()?
            .set_body_pose(body, position, rotation),
        "placed body pose",
    )?;
    let player = world.resource_mut::<Player>()?;
    player.state = state;
    player.yaw = wrap_yaw(yaw);
    let id = player.id;
    write_pose(world, id, position, rotation)
}

/// The character's state after the last tick.
///
/// # Errors
///
/// Returns a world error when the player is missing.
pub fn player_state(world: &World) -> Result<CharacterState, WorldError> {
    Ok(world.resource::<Player>()?.state)
}

/// The character's yaw after the last tick, in radians.
///
/// # Errors
///
/// Returns a world error when the player is missing.
pub fn player_yaw(world: &World) -> Result<f32, WorldError> {
    Ok(world.resource::<Player>()?.yaw)
}

/// The player's physics body: the identifier a test excludes when it queries the scene.
///
/// # Errors
///
/// Returns a world error when the player is missing.
pub fn player_body(world: &World) -> Result<BodyId, WorldError> {
    Ok(world.resource::<Player>()?.body)
}

/// The character configuration: the capsule and the movement constants above.
fn character_config() -> Result<CharacterConfig, PhysicsError> {
    CharacterConfig::capsule(CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M)?
        .skin_width(SKIN_WIDTH_M)?
        .step_up(STEP_HEIGHT_M, STEP_MIN_WIDTH_M)?
        .slope_limit(SLOPE_LIMIT_DEG.to_radians())?
        .ground_snap_distance(GROUND_SNAP_M)?
        .vertical_motion(JUMP_SPEED_MPS, GRAVITY_MPS2, MAX_FALL_SPEED_MPS)
}

/// Spawns the player's entity and its kinematic body, and returns the character resource.
fn spawn_player(world: &mut World, spawn: Vec3) -> Result<Player, WorldError> {
    let config = physics(character_config(), "character config")?;
    let state = physics(CharacterState::new(spawn), "player spawn")?;
    let builder = physics(
        BodyBuilder::kinematic().translation(spawn),
        "player body pose",
    )?;
    let body = {
        let physics_world = world.resource_mut::<PhysicsWorld>()?;
        let body = physics_world.insert_body(builder);
        physics(
            physics_world.insert_collider(Some(body), ColliderBuilder::new(config.shape())),
            "player capsule",
        )?;
        body
    };
    let id = world.spawn()?;
    write_pose(world, id, spawn, Quat::IDENTITY)?;
    Ok(Player {
        controller: CharacterController::new(config),
        state,
        yaw: 0.0,
        id,
        body,
        command: PlayerCommand::IDLE,
    })
}

/// Wraps a yaw into `(-PI, PI]`, so it never grows without bound.
fn wrap_yaw(yaw: f32) -> f32 {
    (yaw + PI).rem_euclid(TAU) - PI
}

/// The world-space ground velocity for a wish at a yaw. Yaw 0 looks down -Z; right is +X.
fn ground_velocity(wish: Vec2, yaw: f32) -> Vec3 {
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    (right * wish.x + forward * wish.y) * WALK_SPEED_MPS
}

/// Turns the pending command into one character move, and drives the body mirror.
fn character_move(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let (movement, body, yaw) = {
        let player = world.resource::<Player>()?;
        let physics_world = world.resource::<PhysicsWorld>()?;
        let yaw = player.command.view.yaw();
        let velocity = ground_velocity(player.command.wish(), yaw);
        let input = physics(
            CharacterInput::new(velocity, player.command.jump),
            "character input",
        )?;
        let movement =
            player
                .controller
                .move_character(player.state, input, physics_world, player.body);
        (movement, player.body, yaw)
    };
    physics(
        world
            .resource_mut::<PhysicsWorld>()?
            .set_next_kinematic_pose(body, movement.state.position(), Quat::from_rotation_y(yaw)),
        "player body pose",
    )?;
    let player = world.resource_mut::<Player>()?;
    player.state = movement.state;
    player.yaw = yaw;
    Ok(())
}

/// Advances the physics world by one fixed step.
fn physics_step(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    world.resource_mut::<PhysicsWorld>()?.step();
    Ok(())
}

/// Copies every scene body's pose and the character's pose into their `Transform`s.
fn sync_transforms(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let entries = world.resource_mut::<Bodies>()?.take();
    let result = write_scene_poses(world, &entries).and_then(|()| write_player_pose(world));
    world.resource_mut::<Bodies>()?.restore(entries);
    result
}

/// Writes each scene body's physics pose into its entity.
fn write_scene_poses(world: &mut World, entries: &[SceneBody]) -> Result<(), WorldError> {
    for entry in entries {
        let state = physics(
            world.resource::<PhysicsWorld>()?.body_state(entry.body),
            "scene body is stale",
        )?;
        write_pose(world, entry.id, state.translation, state.rotation)?;
    }
    Ok(())
}

/// Writes the character's centre and yaw into its entity.
fn write_player_pose(world: &mut World) -> Result<(), WorldError> {
    let (id, centre, yaw) = {
        let player = world.resource::<Player>()?;
        (player.id, player.state.position(), player.yaw)
    };
    write_pose(world, id, centre, Quat::from_rotation_y(yaw))
}
