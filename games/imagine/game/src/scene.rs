//! IMAGINE's arena loader and the game-side interpretation of level shape names.
//!
//! The RON level is validated before any bodies are spawned. Shape names use a role prefix
//! (`floor_`, `wall_`, `crate_`, `building_`, `ramp_`, `platform_`); spawn names use `player_` or
//! `bot_`. This naming convention keeps roles in game data without teaching `engine-scene` about
//! Kiln Yard. Scene entities are stored in spawn order, matching the level file.

use engine_assets::format::AssetId;
use engine_assets::{AssetPath, ContentPack};
use engine_core::StateHasher;
use engine_physics::{BodyId, ConvexShape, PhysicsWorld};
use engine_scene::{
    BodyDef, ColliderMetadata, CueDef, LayoutDef, Level, LevelTransform, MarkerKind,
    PrefabRegistry, PrefabSpawn, SceneError, ShapeDef, ShapeKind, Watched, WatchedLevel,
    install_level_from_content_with_prefabs, installed,
};
use engine_world::{NetId, Replication, Resource, Transform, World};
use glam::{Quat, Vec3};
use kit_fps::{Locomotion, PhysicsSenses, Team, Waypoints};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::data::{CAPSULE_RADIUS_M, CENTRE_ABOVE_FEET_M, EYE_ABOVE_CENTRE_M};
use crate::data::{Tuning, mode_definitions};
use crate::error::downstream;
use crate::rules::{Actors, PLAYER_SLOT};

/// What a level shape is for. The presenter chooses its material; physics treats every shape alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRole {
    /// Arena floor and trench bed.
    Floor,
    /// Perimeter and cover walls.
    Wall,
    /// Breakable-looking crate cover.
    Crate,
    /// Central building pieces.
    Building,
    /// Sloped ramp.
    Ramp,
    /// Raised platform.
    Platform,
}

/// One arena shape entity and its game-side role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneEntry {
    /// Entity with the shape's transform.
    pub id: NetId,
    /// Role decoded from the shape name prefix.
    pub role: BoxRole,
}

/// The arena entities, shape definitions and spawn markers, in file order.
#[derive(Debug, Default)]
pub struct Scene {
    entries: Vec<SceneEntry>,
}

/// Mounted game content required to resolve level records and cooked scene assets.
pub struct SceneContent(pub ContentPack);

impl Resource for SceneContent {
    fn hash_into(&self, _hasher: &mut StateHasher) {}
}

impl Scene {
    /// Arena entries in level order.
    #[must_use]
    pub fn entries(&self) -> &[SceneEntry] {
        &self.entries
    }

    /// Role of an arena entity.
    #[must_use]
    pub fn role_of(&self, id: NetId) -> Option<BoxRole> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.role)
    }
}

impl Resource for Scene {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for entry in &self.entries {
            hasher.write_u32(entry.id.index());
            hasher.write_u32(entry.id.generation());
            hasher.write_u8(entry.role as u8);
        }
    }
}

/// The game markers needed by spawning, bot navigation and respawning.
#[derive(Debug, Default)]
pub struct ArenaData {
    player_spawns: Vec<Vec3>,
    bot_spawns: Vec<Vec3>,
    waypoints: Vec<Vec3>,
    layouts: Vec<LayoutDef>,
    cues: Vec<CueDef>,
    current_layout: Option<String>,
    instance_entities: BTreeMap<AssetId, NetId>,
    collider_bodies: BTreeMap<AssetId, BodyId>,
    transition: Option<LayoutTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct ArenaSwitch(bool);

#[derive(Debug, Clone)]
struct MoverTween {
    id: AssetId,
    entity: NetId,
    start: Transform,
    target: Transform,
}

#[derive(Debug, Clone, Copy)]
struct SwitchTarget {
    entity: NetId,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct LayoutTransition {
    target_layout: String,
    elapsed_s: f32,
    duration_s: f32,
    movers: Vec<MoverTween>,
    switches: Vec<SwitchTarget>,
}

impl ArenaData {
    /// Player spawn points in file order.
    #[must_use]
    pub fn player_spawns(&self) -> &[Vec3] {
        &self.player_spawns
    }
    /// Bot spawn points in file order.
    #[must_use]
    pub fn bot_spawns(&self) -> &[Vec3] {
        &self.bot_spawns
    }
    /// Waypoints in file order.
    #[must_use]
    pub fn waypoints(&self) -> &[Vec3] {
        &self.waypoints
    }

    /// Name of the last layout reached by a cue.
    #[must_use]
    pub fn current_layout(&self) -> Option<&str> {
        self.current_layout.as_deref()
    }
}

impl Resource for ArenaData {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u64(u64::try_from(self.player_spawns.len()).unwrap_or(u64::MAX));
        hasher.write_u64(u64::try_from(self.bot_spawns.len()).unwrap_or(u64::MAX));
        hasher.write_u64(u64::try_from(self.waypoints.len()).unwrap_or(u64::MAX));
        for point in self
            .player_spawns
            .iter()
            .chain(&self.bot_spawns)
            .chain(&self.waypoints)
        {
            hasher.write_f32(point.x);
            hasher.write_f32(point.y);
            hasher.write_f32(point.z);
        }
        for (asset_id, entity) in &self.instance_entities {
            hasher.write_u64(asset_id.get());
            hasher.write_u32(entity.index());
            hasher.write_u32(entity.generation());
        }
        if let Some(layout) = &self.current_layout {
            hasher.write_bytes(layout.as_bytes());
        }
        if let Some(transition) = &self.transition {
            hasher.write_bytes(transition.target_layout.as_bytes());
            hasher.write_f32(transition.elapsed_s);
            hasher.write_f32(transition.duration_s);
            for mover in &transition.movers {
                hasher.write_u64(mover.id.get());
                hash_transform(hasher, mover.start);
                hash_transform(hasher, mover.target);
            }
            for switch in &transition.switches {
                hasher.write_u32(switch.entity.index());
                hasher.write_u32(switch.entity.generation());
                hasher.write_u8(u8::from(switch.enabled));
            }
        }
    }
}

fn hash_transform(hasher: &mut StateHasher, transform: Transform) {
    for value in transform
        .translation()
        .to_array()
        .into_iter()
        .chain(transform.rotation().to_array())
        .chain(transform.scale().to_array())
    {
        hasher.write_f32(value);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HealthBlock {
    hp: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplosiveBlock {
    radius_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DoorBlock {
    open_s: f32,
}

fn spawn_health_block(
    world: &mut World,
    placement: PrefabSpawn,
    block: HealthBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

fn spawn_explosive_block(
    world: &mut World,
    placement: PrefabSpawn,
    block: ExplosiveBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

fn spawn_door_block(
    world: &mut World,
    placement: PrefabSpawn,
    block: DoorBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

/// Registers IMAGINE's game-owned component blocks for the shared prefab loader.
pub(crate) fn register_prefab_components(world: &mut World) -> Result<(), SceneError> {
    world.register::<HealthBlock>(Replication::ServerOnly)?;
    world.register::<ExplosiveBlock>(Replication::ServerOnly)?;
    world.register::<DoorBlock>(Replication::ServerOnly)?;
    world.register::<ArenaSwitch>(Replication::ServerOnly)?;
    Ok(())
}

fn prefab_registry() -> Result<PrefabRegistry, SceneError> {
    let mut registry = PrefabRegistry::default();
    registry.register("health", spawn_health_block)?;
    registry.register("explosive", spawn_explosive_block)?;
    registry.register("door", spawn_door_block)?;
    Ok(registry)
}

/// Builds the game-owned prefab and collision policies used by engine live diffs.
pub fn live_scene_content(content: &ContentPack) -> Result<engine_scene::SceneContent, SceneError> {
    Ok(engine_scene::SceneContent::new(
        content.clone(),
        prefab_registry()?,
        live_collider_metadata,
    ))
}

/// Loads an arena and its cooked dependencies from the mounted game pack.
pub fn start_scene_from_content(world: &mut World, arena_id: u16) -> Result<(), SceneError> {
    let content = world.resource::<SceneContent>()?.0.clone();
    let level = level_from_content(&content, arena_id)?;
    let filename = if arena_id == 0 {
        "data/arenas/kiln_yard.level.ron"
    } else {
        "content/cooked/sample_arena.level.ron"
    };
    let gravity = world.resource::<Tuning>()?.movement.gravity;
    validate_level(&level, filename, gravity)?;
    validate_mode_spawns(&level, filename, &content)?;
    install_scene(world, &level)?;
    let path = AssetPath::new(filename).map_err(|error| SceneError::Tuning {
        file: filename.to_owned(),
        reason: error.to_string(),
    })?;
    let watched = Watched::level(path, &content)?;
    world.insert_resource(WatchedLevel::new(watched))?;
    Ok(())
}

/// Parses a selected arena from logical pack content.
pub fn level_from_content(content: &ContentPack, arena_id: u16) -> Result<Level, SceneError> {
    let filename = match arena_id {
        0 => "data/arenas/kiln_yard.level.ron",
        1 => "content/cooked/sample_arena.level.ron",
        _ => {
            return Err(SceneError::Tuning {
                file: "arena selection".to_owned(),
                reason: format!("unknown arena id {arena_id}"),
            });
        }
    };
    let path = AssetPath::new(filename).map_err(|error| SceneError::Tuning {
        file: filename.to_owned(),
        reason: error.to_string(),
    })?;
    let text = content
        .load_text(&path)
        .map_err(|error| SceneError::Tuning {
            file: filename.to_owned(),
            reason: error.to_string(),
        })?;
    let level = Level::parse(text, filename)?;
    Ok(level)
}

/// Replaces the installed level after validation and updates game-specific role and spawn data.
pub fn install_scene(world: &mut World, level: &Level) -> Result<(), SceneError> {
    let gravity = world.resource::<Tuning>()?.movement.gravity;
    validate_level(level, level.name(), gravity)?;
    let content = world.resource::<SceneContent>()?.0.clone();
    validate_mode_spawns(level, level.name(), &content)?;
    let prefabs = prefab_registry()?;
    install_level_from_content_with_prefabs(world, level, &content, collider_metadata, &prefabs)?;
    let spawned = installed(world)?;
    let instance_entities = level
        .instances()
        .iter()
        .zip(spawned.instances())
        .map(|(instance, entity)| (instance.asset_id, *entity))
        .collect::<BTreeMap<_, _>>();
    let collider_bodies = level
        .colliders()
        .iter()
        .zip(spawned.colliders())
        .map(|(collider, spawned)| (collider.asset_id, spawned.body))
        .collect::<BTreeMap<_, _>>();
    let default_layout = level.layouts().first();
    if let Some(layout) = default_layout {
        for (asset_id, pose) in &layout.mover_poses {
            if let Some(body) = collider_bodies.get(asset_id) {
                let transform = runtime_transform(*pose)?;
                world.resource_mut::<PhysicsWorld>()?.set_body_pose(
                    *body,
                    transform.translation(),
                    transform.rotation(),
                )?;
            }
        }
        for (asset_id, enabled) in &layout.switch_states {
            if let Some(entity) = instance_entities.get(asset_id) {
                world.insert(*entity, ArenaSwitch(*enabled))?;
            }
        }
    }
    let mut scene = Scene::default();
    for (body, shape) in installed(world)?.bodies().iter().zip(level.shapes()) {
        scene.entries.push(SceneEntry {
            id: body.id,
            role: role_of(shape.name())?,
        });
    }
    let markers = installed(world)?.markers();
    let player_spawns = markers
        .list(MarkerKind::Spawn)
        .iter()
        .filter(|marker| marker.name.starts_with("player_") || marker.name.starts_with("spawn_"))
        .map(|marker| Vec3::from_array(marker.translation))
        .collect();
    let bot_spawns = markers
        .list(MarkerKind::Spawn)
        .iter()
        .filter(|marker| marker.name.starts_with("bot_") || marker.name.starts_with("spawn_"))
        .map(|marker| Vec3::from_array(marker.translation))
        .collect();
    let waypoints = markers
        .list(MarkerKind::Waypoint)
        .iter()
        .map(|marker| Vec3::from_array(marker.translation))
        .collect();
    if world.resource::<Scene>().is_err() {
        world.insert_resource(Scene::default())?;
    }
    if world.resource::<ArenaData>().is_err() {
        world.insert_resource(ArenaData::default())?;
    }
    *world.resource_mut::<Scene>()? = scene;
    *world.resource_mut::<ArenaData>()? = ArenaData {
        player_spawns,
        bot_spawns,
        waypoints,
        layouts: level.layouts().to_vec(),
        cues: level.cues().to_vec(),
        current_layout: default_layout.map(|layout| layout.name.clone()),
        instance_entities,
        collider_bodies,
        transition: None,
    };
    if world.resource::<Waypoints>().is_ok() {
        let points = world.resource::<ArenaData>()?.waypoints().to_vec();
        let tuning = world.resource::<Tuning>()?;
        let senses = PhysicsSenses::with_filter(
            world.resource::<PhysicsWorld>()?,
            crate::collision::sightline_query(),
        );
        let graph = kit_fps::error::lift(Waypoints::build(points, &senses, &tuning.link_rules()?))?;
        *world.resource_mut::<Waypoints>()? = graph;
    }
    move_actors_out_of_geometry(world, level)?;
    Ok(())
}

/// Rebuilds IMAGINE's level-derived indexes after the engine has applied a stable-ID diff.
pub fn refresh_live_scene(
    world: &mut World,
    level: &Level,
    content: &ContentPack,
) -> Result<(), SceneError> {
    let gravity = world.resource::<Tuning>()?.movement.gravity;
    validate_level(level, level.name(), gravity)?;
    validate_mode_spawns(level, level.name(), content)?;
    let previous_layout = world.resource::<ArenaData>()?.current_layout.clone();
    let layout_name = previous_layout
        .filter(|name| level.layouts().iter().any(|layout| layout.name == *name))
        .or_else(|| level.layouts().first().map(|layout| layout.name.clone()));
    let spawned = installed(world)?;
    let mut scene = Scene::default();
    for shape in level.shapes() {
        let Some(entry) = spawned.entries().get(&shape.id()) else {
            continue;
        };
        if world.is_alive(entry.id) {
            scene.entries.push(SceneEntry {
                id: entry.id,
                role: role_of(shape.name())?,
            });
        }
    }
    let instance_entities = level
        .instances()
        .iter()
        .filter_map(|record| {
            spawned
                .entries()
                .get(&record.asset_id)
                .filter(|entry| world.is_alive(entry.id))
                .map(|entry| (record.asset_id, entry.id))
        })
        .collect::<BTreeMap<_, _>>();
    let collider_bodies = level
        .colliders()
        .iter()
        .filter_map(|record| {
            spawned
                .entries()
                .get(&record.asset_id)
                .and_then(|entry| entry.body.map(|body| (record.asset_id, body)))
        })
        .collect::<BTreeMap<_, _>>();
    let markers = spawned.markers();
    let player_spawns = markers
        .list(MarkerKind::Spawn)
        .iter()
        .filter(|marker| marker.name.starts_with("player_") || marker.name.starts_with("spawn_"))
        .map(|marker| Vec3::from_array(marker.translation))
        .collect();
    let bot_spawns = markers
        .list(MarkerKind::Spawn)
        .iter()
        .filter(|marker| marker.name.starts_with("bot_") || marker.name.starts_with("spawn_"))
        .map(|marker| Vec3::from_array(marker.translation))
        .collect();
    let waypoints = markers
        .list(MarkerKind::Waypoint)
        .iter()
        .map(|marker| Vec3::from_array(marker.translation))
        .collect::<Vec<_>>();
    *world.resource_mut::<Scene>()? = scene;
    *world.resource_mut::<ArenaData>()? = ArenaData {
        player_spawns,
        bot_spawns,
        waypoints: waypoints.clone(),
        layouts: level.layouts().to_vec(),
        cues: level.cues().to_vec(),
        current_layout: layout_name,
        instance_entities,
        collider_bodies,
        transition: None,
    };
    if world.resource::<Waypoints>().is_ok() {
        let tuning = world.resource::<Tuning>()?;
        let senses = PhysicsSenses::with_filter(
            world.resource::<PhysicsWorld>()?,
            crate::collision::sightline_query(),
        );
        *world.resource_mut::<Waypoints>()? =
            kit_fps::error::lift(Waypoints::build(waypoints, &senses, &tuning.link_rules()?))?;
    }
    Ok(())
}

/// Starts an authored Director cue. Movers interpolate from their live transform and switches
/// change when the transition reaches the target layout.
pub fn activate_layout_cue(world: &mut World, cue_name: &str) -> Result<(), SceneError> {
    let data = world.resource::<ArenaData>()?;
    let cue = data
        .cues
        .iter()
        .find(|cue| cue.name == cue_name)
        .cloned()
        .ok_or_else(|| SceneError::Tuning {
            file: "arena cues".to_owned(),
            reason: format!("unknown cue `{cue_name}`"),
        })?;
    let current_name = data
        .current_layout
        .as_deref()
        .ok_or_else(|| SceneError::Tuning {
            file: "arena layouts".to_owned(),
            reason: "cue requires an authored layout".to_owned(),
        })?;
    let current = data
        .layouts
        .iter()
        .find(|layout| layout.name == current_name)
        .cloned()
        .ok_or_else(|| SceneError::Tuning {
            file: "arena layouts".to_owned(),
            reason: format!("current layout `{current_name}` is missing"),
        })?;
    let target = data
        .layouts
        .iter()
        .find(|layout| layout.name == cue.layout)
        .cloned()
        .ok_or_else(|| SceneError::Tuning {
            file: "arena cues".to_owned(),
            reason: format!("cue `{cue_name}` targets missing layout `{}`", cue.layout),
        })?;
    let entity_ids = data.instance_entities.clone();
    let mut movers = Vec::new();
    for (asset_id, target_pose) in &target.mover_poses {
        if current.mover_poses.get(asset_id) == Some(target_pose) {
            continue;
        }
        let entity = entity_ids
            .get(asset_id)
            .copied()
            .ok_or_else(|| SceneError::Tuning {
                file: "arena layouts".to_owned(),
                reason: format!("mover {} has no runtime instance", asset_id.get()),
            })?;
        let start = *world.get::<Transform>(entity)?;
        movers.push(MoverTween {
            id: *asset_id,
            entity,
            start,
            target: runtime_transform(*target_pose)?,
        });
    }
    let mut switches = Vec::new();
    for (asset_id, enabled) in &target.switch_states {
        if current.switch_states.get(asset_id) == Some(enabled) {
            continue;
        }
        let entity = entity_ids
            .get(asset_id)
            .copied()
            .ok_or_else(|| SceneError::Tuning {
                file: "arena layouts".to_owned(),
                reason: format!("switch {} has no runtime instance", asset_id.get()),
            })?;
        switches.push(SwitchTarget {
            entity,
            enabled: *enabled,
        });
    }
    world.resource_mut::<ArenaData>()?.transition = Some(LayoutTransition {
        target_layout: target.name,
        elapsed_s: 0.0,
        duration_s: cue.duration_s,
        movers,
        switches,
    });
    Ok(())
}

/// Advances an active arena layout cue by one fixed simulation tick.
pub fn tick_layout_transition(world: &mut World) -> Result<(), SceneError> {
    let dt = 1.0 / f32::from(crate::data::TICK_RATE_HZ);
    let mut transition = world.resource_mut::<ArenaData>()?.transition.take();
    let Some(mut transition_value) = transition.take() else {
        return Ok(());
    };
    transition_value.elapsed_s = (transition_value.elapsed_s + dt).min(transition_value.duration_s);
    let alpha = (transition_value.elapsed_s / transition_value.duration_s).clamp(0.0, 1.0);
    for mover in &transition_value.movers {
        let translation = mover
            .start
            .translation()
            .lerp(mover.target.translation(), alpha);
        let rotation = mover.start.rotation().slerp(mover.target.rotation(), alpha);
        let scale = mover.start.scale().lerp(mover.target.scale(), alpha);
        let mut transform = world.get_mut::<Transform>(mover.entity)?;
        transform.set_translation(translation)?;
        transform.set_rotation(rotation)?;
        transform.set_scale(scale)?;
        drop(transform);
        if let Some(body) = world
            .resource::<ArenaData>()?
            .collider_bodies
            .get(&mover.id)
            .copied()
        {
            world
                .resource_mut::<PhysicsWorld>()?
                .set_body_pose(body, translation, rotation)?;
        }
    }
    if alpha >= 1.0 {
        for switch in &transition_value.switches {
            world.insert(switch.entity, ArenaSwitch(switch.enabled))?;
        }
        world.resource_mut::<ArenaData>()?.current_layout = Some(transition_value.target_layout);
    } else {
        world.resource_mut::<ArenaData>()?.transition = Some(transition_value);
    }
    Ok(())
}

fn runtime_transform(transform: LevelTransform) -> Result<Transform, SceneError> {
    let [yaw, pitch, roll] = transform.rotation_deg.map(f32::to_radians);
    Ok(Transform::new(
        Vec3::from_array(transform.translation),
        Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll),
        Vec3::from_array(transform.scale),
    )?)
}

fn role_of(name: Option<&str>) -> Result<BoxRole, SceneError> {
    let name = name.ok_or_else(|| SceneError::Tuning {
        file: "level".into(),
        reason: "every shape needs a role name".into(),
    })?;
    let role = if name.starts_with("floor_") {
        BoxRole::Floor
    } else if name.starts_with("wall_") {
        BoxRole::Wall
    } else if name.starts_with("crate_") {
        BoxRole::Crate
    } else if name.starts_with("building_") {
        BoxRole::Building
    } else if name.starts_with("ramp_") {
        BoxRole::Ramp
    } else if name.starts_with("platform_") {
        BoxRole::Platform
    } else {
        return Err(SceneError::Tuning {
            file: "level".into(),
            reason: format!("shape `{name}` needs a known role prefix"),
        });
    };
    Ok(role)
}

fn collider_metadata(shape: &ShapeDef, cooked_surface: Option<&str>) -> ColliderMetadata {
    let name = shape.name().unwrap_or_default();
    let is_trigger = name.starts_with("trigger_");
    let layers = if is_trigger {
        crate::collision::trigger_layers()
    } else if name.starts_with("crate_") {
        crate::collision::prop_layers()
    } else {
        crate::collision::world_layers()
    };
    ColliderMetadata {
        layers,
        surface: cooked_surface.map_or_else(
            || crate::collision::surface(shape.look().material),
            crate::collision::surface_name,
        ),
        sensor: is_trigger,
    }
}

fn live_collider_metadata(
    shape: Option<&ShapeDef>,
    cooked_surface: Option<&str>,
) -> ColliderMetadata {
    shape.map_or_else(ColliderMetadata::default, |shape| {
        collider_metadata(shape, cooked_surface)
    })
}

fn validate_level(level: &Level, file: &str, gravity: f32) -> Result<(), SceneError> {
    if level.gravity() != [0.0, -gravity, 0.0] {
        return Err(SceneError::Tuning {
            file: file.into(),
            reason: "gravity must match IMAGINE movement.gravity".into(),
        });
    }
    let players = level
        .spawns()
        .iter()
        .filter(|p| p.name.starts_with("player_"))
        .count();
    if players == 0 {
        return Err(SceneError::Tuning {
            file: file.into(),
            reason: "spawns.player must contain at least one spawn".into(),
        });
    }
    if level.waypoints().is_empty() {
        return Err(SceneError::Tuning {
            file: file.into(),
            reason: "waypoints must not be empty".into(),
        });
    }
    for (index, shape) in level.shapes().iter().enumerate() {
        role_of(shape.name()).map_err(|error| SceneError::Tuning {
            file: file.into(),
            reason: format!("shapes[{index}]: {error}"),
        })?;
    }
    for marker in level.spawns().iter().chain(level.waypoints()) {
        if !marker_clear(level, Vec3::from_array(marker.translation)) {
            return Err(SceneError::Tuning {
                file: file.into(),
                reason: format!("marker `{}` stands inside a solid", marker.name),
            });
        }
    }
    Ok(())
}

fn validate_mode_spawns(
    level: &Level,
    file: &str,
    content: &ContentPack,
) -> Result<(), SceneError> {
    let bots = level
        .spawns()
        .iter()
        .filter(|spawn| spawn.name.starts_with("bot_"))
        .count();
    let modes_path = AssetPath::new("data/modes.ron").map_err(|error| SceneError::Tuning {
        file: file.to_owned(),
        reason: error.to_string(),
    })?;
    let modes_text = content
        .load_text(&modes_path)
        .map_err(|error| SceneError::Tuning {
            file: file.to_owned(),
            reason: error.to_string(),
        })?;
    for mode in mode_definitions(modes_text).map_err(|reason| SceneError::Tuning {
        file: file.into(),
        reason,
    })? {
        if mode.available && bots < usize::from(mode.capacity.max_bots()) {
            return Err(SceneError::Tuning {
                file: file.into(),
                reason: format!(
                    "mode {} needs {} bot spawn markers but the arena has {bots}",
                    mode.name,
                    mode.capacity.max_bots()
                ),
            });
        }
    }
    Ok(())
}

fn marker_clear(level: &Level, feet: Vec3) -> bool {
    [0.1, CENTRE_ABOVE_FEET_M, 1.6].iter().all(|height| {
        let point = feet + Vec3::Y * *height;
        level.shapes().iter().all(|shape| {
            if !matches!(shape.body(), BodyDef::Static | BodyDef::Kinematic) {
                return true;
            }
            let local = Quat::from_rotation_z(-shape.rotation_deg()[2].to_radians())
                * (point - Vec3::from_array(shape.translation()));
            match shape.kind() {
                ShapeKind::Cuboid { half } | ShapeKind::Ramp { half, .. } => {
                    !local.abs().cmple(Vec3::from_array(half)).all()
                }
                ShapeKind::Sphere { radius } => local.length_squared() > radius * radius,
                ShapeKind::Capsule { .. } => true,
            }
        })
    })
}

fn move_actors_out_of_geometry(world: &mut World, level: &Level) -> Result<(), SceneError> {
    if world.resource::<Actors>().is_err() {
        return Ok(());
    }
    let arena = world.resource::<ArenaData>()?;
    let spawns = arena.player_spawns.clone();
    let bot_spawns = arena.bot_spawns.clone();
    let actors = world.resource::<Actors>()?.slots().to_vec();
    for (slot, actor) in actors.iter().enumerate() {
        let locomotion = *world.get::<Locomotion>(actor.id())?;
        let tuning = world.resource::<Tuning>()?.movement;
        let current_height = if matches!(
            locomotion.move_state(),
            kit_fps::MoveState::Crouch | kit_fps::MoveState::Slide { .. }
        ) {
            tuning.crouch_height
        } else {
            tuning.stand_height
        };
        if marker_clear(
            level,
            locomotion.centre() - Vec3::Y * (current_height * 0.5),
        ) {
            continue;
        }
        let team = world.get::<Team>(actor.id())?.number();
        let points = if slot == PLAYER_SLOT {
            &spawns
        } else {
            &bot_spawns
        };
        let nearest = points
            .iter()
            .min_by(|a, b| {
                a.distance_squared(locomotion.centre())
                    .total_cmp(&b.distance_squared(locomotion.centre()))
            })
            .copied()
            .ok_or_else(|| SceneError::Tuning {
                file: level.name().into(),
                reason: format!("team {team} has no spawn"),
            })?;
        let centre = nearest + Vec3::Y * (tuning.stand_height * 0.5);
        let standing_shape = ConvexShape::capsule(
            tuning.stand_height * 0.5 - CAPSULE_RADIUS_M,
            CAPSULE_RADIUS_M,
        )
        .map_err(downstream)?;
        world
            .resource_mut::<PhysicsWorld>()?
            .set_collider_shape(locomotion.collider(), standing_shape)
            .map_err(downstream)?;
        world
            .resource_mut::<PhysicsWorld>()?
            .set_body_pose(locomotion.body(), centre, Quat::IDENTITY)
            .map_err(downstream)?;
        let mut reset = kit_fps::error::lift(Locomotion::new(
            locomotion.body(),
            locomotion.collider(),
            centre,
            EYE_ABOVE_CENTRE_M,
        ))?;
        reset.set_command(locomotion.command());
        *world.get_mut::<Locomotion>(actor.id())? = reset;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn embedded_kiln_yard_parses_and_validates() {
        let content = crate::test_support::pack().expect("test pack");
        level_from_content(&content, 0).expect("packed Kiln Yard is valid");
    }

    #[test]
    fn a_mode_without_enough_bot_spawns_is_named_in_the_validation_error() {
        let content = crate::test_support::pack().expect("test pack");
        let path = AssetPath::new("data/arenas/kiln_yard.level.ron").expect("level path");
        let marker = "        (name: \"bot_5\", translation: (-23.0, 0.0, -23.0), yaw_deg: 0.0),\n";
        let source = content
            .load_text(&path)
            .expect("packed level")
            .replace(marker, "");
        let level = Level::parse(&source, "short-spawns.level.ron").expect("level parses");
        let error = validate_mode_spawns(&level, "short-spawns.level.ron", &content)
            .expect_err("the practice capacity requires six bot spawns");
        assert!(error.to_string().contains("PRACTICE"));
    }

    #[test]
    fn generated_sample_arena_is_selectable_and_resolves_its_cooked_assets() {
        let content = crate::test_support::pack().expect("test pack");
        let level = level_from_content(&content, 1).expect("sample arena is valid");
        assert_eq!(
            level.layouts().first().map(|layout| layout.name.as_str()),
            Some("open")
        );
        assert!(level.cues().iter().any(|cue| cue.name == "lockdown"));
        assert_eq!(level.prefabs().len(), 5);
        let index_path = AssetPath::new("content/cooked/index.ron").expect("index path");
        let index_text = content.load_text(&index_path).expect("sample cooked index");
        let index: engine_assets::format::AssetIndex =
            ron::from_str(index_text).expect("sample cooked index syntax");
        for id in level
            .instances()
            .iter()
            .map(|record| record.asset_id)
            .chain(level.colliders().iter().map(|record| record.asset_id))
            .chain(level.prefabs().iter().map(|record| record.prefab_id))
            .chain(level.volumes().iter().map(|record| record.id))
        {
            assert!(index.contains_key(&id), "unresolved asset {}", id.get());
        }
    }
}
