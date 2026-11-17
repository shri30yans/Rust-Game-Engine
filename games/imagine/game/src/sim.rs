//! The arena's simulation: the game registration, the actors, the schedule and the transform sync.
//!
//! Actors are the player (slot 0) and the configured bots (slots 1 onward). Each is a kinematic
//! capsule driven by the kit's [`Locomotion`], with a team, health, a rifle and, for bots, a brain.
//! Input enters only through [`set_player_command`]. The physics world has zero gravity, because
//! the kit's motor applies its own.
//!
//! Each tick runs, in the order the data flows (PLAN §3.1): `fps.bot` (commands in),
//! `arena.layouts` (movers pose), `fps.locomotion` (characters move), `physics.step`, `fps.weapon`,
//! `fps.damage`, `rules.respawn`, then the readers of the tick's events ([`EVENT_READERS`]:
//! `fps.combat_stats`, `feedback.capture`, `bots.hear`, `rules.update`) and `sync.transforms`. The
//! engine schedule empties every event log before the tick, so no system clears one and the
//! readers may run in any order.
//!
//! Invariants: the schedule order above; actor slots never change; the transform sync allocates
//! nothing per tick.

use app::{AppConfig, AppError, Game, GameInfo, NetGame, PlayerId, TickRate};
use engine_assets::ContentPack;
use engine_netcode::{DEFAULT_CHARACTER_MOVEMENT_PRECISION_M, TransformBounds};
use engine_physics::{BodyBuilder, ColliderBuilder, ConvexShape, PhysicsConfig, PhysicsWorld};
use engine_world::{NetId, Schedule, SystemContext, SystemFn, Transform, World, WorldError};
use glam::{Quat, Vec3};
use kit_fps::{
    Bot, BotConfig, CombatStats, Health, Locomotion, Motor, MoveCommand, PhysicsSenses, Team,
    Waypoints, Weapon, bot_system, combat_stats_system, damage_system, locomotion_system,
    weapon_system,
};
use serde::{Deserialize, Serialize};

use crate::data::{
    CAPSULE_RADIUS_M, EYE_ABOVE_CENTRE_M, TICK_RATE_HZ, Tuning, load_modes, load_tuning,
};
use crate::error::downstream;
use crate::feedback::{HudEvents, capture_feedback};
use crate::hearing::hear_shots;
use crate::rules::{
    Actor, Actors, KillFeed, Life, PLAYER_SLOT, Score, respawn_system, rules_system,
};
use crate::scene::{ArenaData, start_scene_from_content};

/// The team of the player.
const PLAYER_TEAM: u8 = 0;
/// Room for every entity: the scene's boxes, the actors and the feedback entities.
const MAX_ENTITIES: usize = 256;
/// Extra network travel room around the authored arena geometry, in metres.
const TRANSFORM_BOUNDS_MARGIN_M: f32 = 10.0;

/// The systems that read the tick's kit events after the damage pass. Each writes only its own
/// state, so they may run in any order; a match test runs every order and compares the results.
pub const EVENT_READERS: [(&str, SystemFn); 4] = [
    ("fps.combat_stats", combat_stats_system),
    ("feedback.capture", capture_feedback),
    ("bots.hear", hear_shots),
    ("rules.update", rules_system),
];

/// The arena game: registers the kit, the physics world, the scene and the actors, and adds the
/// systems in their order.
pub struct ImagineGame {
    content: ContentPack,
}

impl ImagineGame {
    /// Creates IMAGINE from its mounted content pack.
    #[must_use]
    pub fn from_content(content: &ContentPack) -> Self {
        Self {
            content: content.clone(),
        }
    }
}

impl Game for ImagineGame {
    type Command = MoveCommand;
    type Settings = ImagineSettings;

    fn dynamic_entity_budget(&self) -> usize {
        MAX_ENTITIES
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
        crate::scene::refresh_live_scene(world, level, content)
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "imagine",
            protocol_id: 2,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        settings: &Self::Settings,
    ) -> Result<(), AppError> {
        setup_arena_with_content(world, schedule, &self.content, settings)?;
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        player: PlayerId,
        _name: &engine_sessions::PlayerName,
    ) -> Result<NetId, AppError> {
        let slot = if player == PlayerId::LOCAL {
            PLAYER_SLOT
        } else {
            usize::try_from(player.seat().raw())
                .map_err(|_| WorldError::Invariant("player seat does not fit an actor slot"))?
        };
        while world.resource::<Actors>()?.slots().len() <= slot {
            spawn_joined_actor(world)?;
        }
        let actor = world
            .resource::<Actors>()?
            .get(slot)
            .ok_or(WorldError::Invariant("joined actor slot is missing"))?;
        Ok(actor.id())
    }

    fn leave(&self, _world: &mut World, _player: PlayerId) -> Result<(), AppError> {
        Ok(())
    }

    fn command(
        &self,
        world: &mut World,
        player: PlayerId,
        command: &Self::Command,
    ) -> Result<(), AppError> {
        let slot = if player == PlayerId::LOCAL {
            PLAYER_SLOT
        } else {
            usize::try_from(player.seat().raw())
                .map_err(|_| WorldError::Invariant("player seat does not fit an actor slot"))?
        };
        let Some(actor) = world.resource::<Actors>()?.get(slot) else {
            return Ok(());
        };
        if !actor.is_alive() {
            return Ok(());
        }
        world
            .get_mut::<Locomotion>(actor.id())?
            .set_command(*command);
        Ok(())
    }

    fn settings_for_match(
        &self,
        settings: &Self::Settings,
        bot_seats: usize,
    ) -> Result<Self::Settings, AppError> {
        let bot_count = u8::try_from(bot_seats)
            .map_err(|_| WorldError::Invariant("match bot count exceeds u8"))?;
        Ok(ImagineSettings {
            arena_id: settings.arena_id,
            mode: settings.mode,
            menu_backdrop: false,
            bot_teams: (1..=bot_count).collect(),
        })
    }
}

/// Lobby-selected settings for an IMAGINE match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagineSettings {
    /// Game-owned arena identifier.
    pub arena_id: u16,
    /// Mode selected in the room. Only Practice is currently playable.
    pub mode: ImagineMode,
    /// Whether this world is the front-end backdrop with no match bots.
    pub menu_backdrop: bool,
    /// Team number for each bot, in actor-slot order.
    pub bot_teams: Vec<u8>,
}

/// A game mode that the current game build can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImagineMode {
    /// A free-practice arena with bots.
    Practice,
}

impl ImagineSettings {
    /// Settings for the front-end arena backdrop.
    #[must_use]
    pub fn menu_backdrop() -> Self {
        Self {
            arena_id: 0,
            mode: ImagineMode::Practice,
            menu_backdrop: true,
            bot_teams: Vec::new(),
        }
    }
}

impl Default for ImagineSettings {
    fn default() -> Self {
        Self {
            arena_id: 0,
            mode: ImagineMode::Practice,
            menu_backdrop: false,
            bot_teams: (1..=6).collect(),
        }
    }
}

/// IMAGINE's locally predicted character movement.
pub struct ImaginePrediction;

impl engine_netcode::Predicted for ImaginePrediction {
    type State = Locomotion;
    type Input = MoveCommand;

    fn capture(world: &World, id: NetId) -> Result<Self::State, engine_netcode::NetError> {
        world
            .get::<Locomotion>(id)
            .map(|value| *value)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))
    }

    fn restore(
        world: &mut World,
        id: NetId,
        state: &Self::State,
    ) -> Result<(), engine_netcode::NetError> {
        let rotation = Quat::from_rotation_y(state.command().view().yaw());
        world
            .resource_mut::<PhysicsWorld>()
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?
            .set_body_pose(state.body(), state.centre(), rotation)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?;
        world
            .insert(id, *state)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))
    }

    fn step(
        world: &mut World,
        id: NetId,
        input: &Self::Input,
    ) -> Result<(), engine_netcode::NetError> {
        world
            .get_mut::<Locomotion>(id)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?
            .set_command(*input);
        kit_fps::move_one(world, id, 1.0 / f32::from(TICK_RATE_HZ))
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?;
        world
            .resource_mut::<PhysicsWorld>()
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?
            .step();
        let locomotion = *world
            .get::<Locomotion>(id)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?;
        let mut transform = world
            .get_mut::<Transform>(id)
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?;
        transform
            .set_translation(locomotion.centre())
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))?;
        transform
            .set_rotation(Quat::from_rotation_y(input.view().yaw()))
            .map_err(|error| engine_netcode::NetError::World(error.to_string()))
    }

    fn position(state: &Self::State) -> Vec3 {
        state.centre()
    }
}

impl NetGame for ImagineGame {
    type Predicted = ImaginePrediction;

    fn replication(
        &self,
        _world: &World,
        _viewer: PlayerId,
        _entity: NetId,
    ) -> engine_netcode::EntityReplication {
        engine_netcode::EntityReplication::always(1.0)
    }

    fn transform_bounds(&self, settings: &Self::Settings) -> Result<TransformBounds, AppError> {
        let level = crate::scene::level_from_content(&self.content, settings.arena_id)?;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for shape in level.shapes() {
            let center = Vec3::from_array(shape.translation());
            let radius = match shape.kind() {
                engine_scene::ShapeKind::Cuboid { half }
                | engine_scene::ShapeKind::Ramp { half, .. } => Vec3::from_array(half).length(),
                engine_scene::ShapeKind::Sphere { radius } => radius,
                engine_scene::ShapeKind::Capsule {
                    half_height,
                    radius,
                } => half_height + radius,
            };
            let extent = Vec3::splat(radius + TRANSFORM_BOUNDS_MARGIN_M);
            min = min.min(center - extent);
            max = max.max(center + extent);
        }
        for transform in level
            .instances()
            .iter()
            .map(|instance| instance.transform)
            .chain(level.prefabs().iter().map(|prefab| prefab.transform))
        {
            let center = Vec3::from_array(transform.translation);
            let radius = Vec3::from_array(transform.scale).length() + TRANSFORM_BOUNDS_MARGIN_M;
            let extent = Vec3::splat(radius);
            min = min.min(center - extent);
            max = max.max(center + extent);
        }
        if !min.is_finite() || !max.is_finite() {
            return Err(WorldError::Invariant("arena has no geometry for transform bounds").into());
        }
        Ok(TransformBounds::new(
            min,
            max,
            DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
        )?)
    }

    fn protocol(&self) -> engine_netcode::GameProtocol {
        let info = self.info();
        engine_netcode::GameProtocol {
            id: engine_netcode::GameId::new(u64::from(info.protocol_id)),
            version: engine_netcode::GameVersion::new(u32::from(info.protocol_version)),
        }
    }
}

/// Registers the arena and its systems with `world` and `schedule`.
///
/// # Errors
///
/// Returns a world error when a component or resource is already registered, a scene body is
/// rejected by the physics world, or a system is already scheduled.
/// Sets up an arena using the supplied match configuration.
pub fn setup_arena_with_settings(
    world: &mut World,
    schedule: &mut Schedule,
    content: &ContentPack,
    settings: &ImagineSettings,
) -> Result<(), WorldError> {
    setup_arena_with_content(world, schedule, content, settings)
}

fn setup_arena_with_content(
    world: &mut World,
    schedule: &mut Schedule,
    content: &ContentPack,
    settings: &ImagineSettings,
) -> Result<(), WorldError> {
    let tuning =
        load_tuning(content).map_err(|_| WorldError::Invariant("IMAGINE tuning failed to load"))?;
    let bot_count = settings.bot_teams.len();
    if settings.mode != ImagineMode::Practice || settings.menu_backdrop && bot_count != 0 {
        return Err(WorldError::Invariant("unsupported IMAGINE world settings"));
    }
    if bot_count > tuning.actors.bot_roster.len() {
        return Err(WorldError::Invariant(
            "match bot count exceeds the bot roster",
        ));
    }
    tuning
        .validate_fields()
        .map_err(|_| WorldError::Invariant("IMAGINE tuning failed validation"))?;
    world.insert_resource(tuning.clone())?;
    world.insert_resource(crate::scene::SceneContent(content.clone()))?;
    // Event logs are sized once for the largest mode, so a session that fills its seats never
    // grows them mid-match.
    let mode_players = load_modes(content)
        .map_err(|_| WorldError::Invariant("IMAGINE modes failed to load"))?
        .iter()
        .map(|mode| usize::from(mode.capacity.max_players))
        .max()
        .unwrap_or(0);
    kit_fps::install(world, mode_players.max(bot_count + 1))?;
    let config = PhysicsConfig::new(schedule.dt(), Vec3::ZERO).map_err(downstream)?;
    world.insert_resource(PhysicsWorld::new(config))?;
    let motor = kit_fps::error::lift(Motor::new(tuning.controller_config()?, CAPSULE_RADIUS_M))?;
    world.insert_resource(motor)?;
    engine_scene::register_components(world)
        .map_err(|_| WorldError::Invariant("scene components failed to register"))?;
    crate::scene::register_prefab_components(world)
        .map_err(|_| WorldError::Invariant("prefab components failed to register"))?;
    start_scene_from_content(world, settings.arena_id)
        .map_err(|_| WorldError::Invariant("IMAGINE arena failed to load"))?;
    // The link graph is built from the arena's static geometry, before any actor is spawned.
    let nav = {
        let senses = PhysicsSenses::with_filter(
            world.resource::<PhysicsWorld>()?,
            crate::collision::sightline_query(),
        );
        kit_fps::error::lift(Waypoints::build(
            world.resource::<ArenaData>()?.waypoints().to_vec(),
            &senses,
            &tuning.link_rules()?,
        ))?
    };
    world.insert_resource(nav)?;
    world.insert_resource(Score::with_actor_count(bot_count + 1))?;
    world.insert_resource(KillFeed::with_rows(crate::rules::KILL_FEED_ROWS))?;
    world.insert_resource(HudEvents::new(bot_count + 1))?;
    let actors = spawn_actors(world, &settings.bot_teams)?;
    let combat_roster: Vec<NetId> = actors.slots().iter().map(|actor| actor.id()).collect();
    world.insert_resource(actors)?;
    world.insert_resource(CombatStats::new(&combat_roster))?;

    add_arena_systems(schedule, &EVENT_READERS)
}

/// Adds the arena's systems to `schedule` in data-flow order, with the event readers in the order
/// given (every order is valid; [`EVENT_READERS`] is the one the game uses).
///
/// # Errors
///
/// Returns a world error when a system is already scheduled.
pub fn add_arena_systems(
    schedule: &mut Schedule,
    readers: &[(&'static str, SystemFn)],
) -> Result<(), WorldError> {
    schedule.add_system("fps.bot", bot_system)?;
    schedule.add_system("arena.layouts", layout_transition_system)?;
    schedule.add_system("fps.locomotion", locomotion_system)?;
    schedule.add_system("physics.step", physics_step)?;
    schedule.add_system("fps.weapon", weapon_system)?;
    schedule.add_system("fps.damage", damage_system)?;
    schedule.add_system("rules.respawn", respawn_system)?;
    for (name, system) in readers {
        schedule.add_system(name, *system)?;
    }
    schedule.add_system("sync.transforms", sync_transforms)?;
    Ok(())
}

/// The app configuration the arena runs with: the data tick rate, seed 1, eight catch-up ticks,
/// room for 256 entities. `job_workers` overrides the device's worker count when `Some`.
///
/// # Errors
///
/// Returns an error when the fixed tick rate is rejected (it cannot be, for `TICK_RATE_HZ`).
pub fn app_config(job_workers: Option<usize>) -> Result<AppConfig, AppError> {
    Ok(AppConfig {
        tick_rate: TickRate::new(TICK_RATE_HZ)?,
        seed: 1,
        max_catch_up_ticks: 8,
        job_workers,
    })
}

/// Sets the player's command for the next tick. A downed player gets an idle command, so input
/// cannot move or fire the player while the respawn timer runs.
///
/// # Errors
///
/// Returns a world error when the player is missing.
pub fn set_player_command(world: &mut World, command: MoveCommand) -> Result<(), WorldError> {
    let player = actor_at(world, PLAYER_SLOT)?;
    let applied = if player.is_alive() {
        command
    } else {
        MoveCommand::IDLE
    };
    world
        .get_mut::<Locomotion>(player.id())?
        .set_command(applied);
    Ok(())
}

/// Applies validated tuning to every live kit component while retaining actor state.
///
/// # Errors
///
/// Returns a world error when tuning, actors or components are missing, or when the kit refuses a
/// controller rebuild.
pub fn apply_tuning(world: &mut World, tuning: Tuning) -> Result<(), WorldError> {
    let controller = tuning.controller_config()?;
    let weapon = tuning.weapon_config()?;
    let bot = tuning.bot_config()?;
    let navigation = {
        let points = world.resource::<ArenaData>()?.waypoints().to_vec();
        let senses = PhysicsSenses::with_filter(
            world.resource::<PhysicsWorld>()?,
            crate::collision::sightline_query(),
        );
        kit_fps::error::lift(Waypoints::build(points, &senses, &tuning.link_rules()?))?
    };
    kit_fps::error::lift(world.resource_mut::<Motor>()?.set_config(controller))?;
    let actors = world.resource::<Actors>()?.slots().to_vec();
    for actor in actors {
        let mut locomotion = *world.get::<Locomotion>(actor.id())?;
        let crouched = matches!(
            locomotion.move_state(),
            kit_fps::MoveState::Crouch | kit_fps::MoveState::Slide { .. }
        );
        let new_height = if crouched {
            tuning.movement.crouch_height
        } else {
            tuning.movement.stand_height
        };
        let shape = ConvexShape::capsule(new_height * 0.5 - CAPSULE_RADIUS_M, CAPSULE_RADIUS_M)
            .map_err(downstream)?;
        world
            .resource_mut::<PhysicsWorld>()?
            .set_collider_shape(locomotion.collider(), shape)
            .map_err(downstream)?;
        let body = world
            .resource::<PhysicsWorld>()?
            .body_state(locomotion.body())
            .map_err(downstream)?;
        kit_fps::error::lift(locomotion.sync_centre(body.translation))?;
        *world.get_mut::<Locomotion>(actor.id())? = locomotion;
        world.get_mut::<Weapon>(actor.id())?.set_config(weapon);
        if let Ok(mut current) = world.get_mut::<Bot>(actor.id()) {
            current.set_config(bot);
        }
    }
    *world.resource_mut::<Waypoints>()? = navigation;
    *world.resource_mut::<Tuning>()? = tuning;
    Ok(())
}

/// Spawns the player and the bots, and returns them in slot order.
fn spawn_actors(world: &mut World, bot_teams: &[u8]) -> Result<Actors, WorldError> {
    let tuning = world.resource::<Tuning>()?.clone();
    let arena = world.resource::<ArenaData>()?;
    let player_spawn = arena
        .player_spawns()
        .first()
        .copied()
        .ok_or(WorldError::Invariant("player spawn missing"))?;
    let bot_spawns = arena.bot_spawns().to_vec();
    let mut slots = Vec::with_capacity(bot_teams.len() + 1);
    // Spawn points are where the feet stand; the body's centre sits above them.
    let lift = Vec3::Y * (tuning.movement.stand_height * 0.5);
    slots.push(spawn_actor(
        world,
        player_spawn + lift,
        PLAYER_TEAM,
        tuning.actors.player_health,
        None,
        0,
    )?);
    // Free for all: every actor is its own team (the player's is 0, the bots' follow), so bots
    // shoot each other as readily as the player.
    for (team, spawn) in bot_teams
        .iter()
        .copied()
        .zip(bot_spawns.into_iter().take(bot_teams.len()))
    {
        let stream = u64::from(team);
        slots.push(spawn_actor(
            world,
            spawn + lift,
            team,
            tuning.actors.bot_health,
            Some(tuning.bot_config()?),
            stream,
        )?);
    }
    Actors::new(slots)
}

/// Adds a non-bot actor for a lobby seat beyond the actors created by the initial tuning.
fn spawn_joined_actor(world: &mut World) -> Result<(), WorldError> {
    let slot = world.resource::<Actors>()?.slots().len();
    let arena = world.resource::<ArenaData>()?;
    let spawns = arena.player_spawns();
    let spawn_index = slot.saturating_sub(1) % spawns.len().max(1);
    let feet = spawns
        .get(spawn_index)
        .copied()
        .ok_or(WorldError::Invariant("player spawn missing"))?;
    let team = u8::try_from(slot)
        .map_err(|_| WorldError::Invariant("actor slot does not fit a team id"))?;
    let lift = Vec3::Y * (world.resource::<Tuning>()?.movement.stand_height * 0.5);
    let actor = spawn_actor(
        world,
        feet + lift,
        team,
        world.resource::<Tuning>()?.actors.player_health,
        None,
        u64::from(team),
    )?;
    world.resource_mut::<Actors>()?.push(actor);
    Ok(())
}

/// Spawns one actor at `centre`: a kinematic capsule body and its entity with the kit's components.
/// A bot also gets a brain, seeded from the world's RNG on its own stream.
fn spawn_actor(
    world: &mut World,
    centre: Vec3,
    team: u8,
    health: u32,
    brain: Option<BotConfig>,
    stream: u64,
) -> Result<Actor, WorldError> {
    let stand_height = world.resource::<Tuning>()?.movement.stand_height;
    let (body, collider) = {
        let physics = world.resource_mut::<PhysicsWorld>()?;
        let builder = BodyBuilder::kinematic()
            .translation(centre)
            .map_err(downstream)?;
        let body = physics.insert_body(builder);
        let shape = ConvexShape::capsule(stand_height * 0.5 - CAPSULE_RADIUS_M, CAPSULE_RADIUS_M)
            .map_err(downstream)?;
        let collider = physics
            .insert_collider(
                Some(body),
                ColliderBuilder::new(shape).layers(crate::collision::character_layers()),
            )
            .map_err(downstream)?;
        (body, collider)
    };
    let id = world.spawn()?;
    world.insert(
        id,
        kit_fps::error::lift(Locomotion::new(body, collider, centre, EYE_ABOVE_CENTRE_M))?,
    )?;
    world.insert(id, Team::new(team))?;
    world.insert(id, kit_fps::error::lift(Health::full(health))?)?;
    world.insert(
        id,
        Weapon::new(world.resource::<Tuning>()?.weapon_config()?),
    )?;
    if let Some(config) = brain {
        let rng = world.rng().fork(stream);
        world.insert(id, Bot::new(config, rng))?;
    }
    Ok(Actor::new(id, body, stream))
}

/// The actor in `slot`, or a world error when the slot does not exist.
fn actor_at(world: &World, slot: usize) -> Result<Actor, WorldError> {
    world
        .resource::<Actors>()?
        .get(slot)
        .ok_or(WorldError::Invariant("actor slot is out of range"))
}

/// Advances the physics world by one fixed step.
fn physics_step(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    world.resource_mut::<PhysicsWorld>()?.step();
    Ok(())
}

fn layout_transition_system(
    world: &mut World,
    _ctx: &mut SystemContext<'_>,
) -> Result<(), WorldError> {
    crate::scene::tick_layout_transition(world)
        .map_err(|_| WorldError::Invariant("arena layout transition failed"))
}

/// Copies each actor's centre and facing into its `Transform`. Allocates nothing: it reads the
/// actor slots by value and writes in place.
pub fn sync_transforms(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let count = world.resource::<Actors>()?.slots().len();
    for slot in 0..count {
        let id = actor_at(world, slot)?.id();
        let (centre, yaw) = {
            let locomotion = world.get::<Locomotion>(id)?;
            (locomotion.centre(), locomotion.command().view().yaw())
        };
        let mut transform = world.get_mut::<Transform>(id)?;
        transform.set_translation(centre)?;
        transform.set_rotation(Quat::from_rotation_y(yaw))?;
    }
    Ok(())
}

/// Whether the actor at `slot` is alive, read from the actor list.
///
/// # Errors
///
/// Returns a world error when the actor list is missing or the slot is out of range.
pub fn is_alive(world: &World, slot: usize) -> Result<bool, WorldError> {
    Ok(matches!(actor_at(world, slot)?.life(), Life::Alive))
}

/// The entity of the actor at `slot`.
///
/// # Errors
///
/// Returns a world error when the actor list is missing or the slot is out of range.
pub fn actor_id(world: &World, slot: usize) -> Result<NetId, WorldError> {
    Ok(actor_at(world, slot)?.id())
}
