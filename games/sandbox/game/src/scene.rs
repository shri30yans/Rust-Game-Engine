//! The sandbox's scene glue. The scene itself is data in the mounted pack at
//! `game/levels/sandbox.ron`. This module only:
//! - loads a level with the sandbox's own check (gravity must match the character's);
//! - starts the level once, installs it and keeps the watch of its file (see [`start_level`]);
//! - installs a level into the world and records each body in [`Bodies`] with its role;
//! - keeps the box helpers ([`BoxSpec`], [`ramp`], [`spawn_box`]) the movement tests spawn with,
//!   each one a thin wrapper over the engine's `spawn_shape`.
//!
//! Invariants: a level is accepted only when its gravity equals the character's gravity; `Bodies`
//! lists every scene body in spawn order and is rebuilt on every install; the spawn point is above
//! the floor's top face.

use engine_assets::format::AssetId;
use engine_assets::{AssetPath, ContentPack};
use engine_core::StateHasher;
use engine_physics::PhysicsError;
use engine_scene::{
    BodyDef, LayoutDef, Level, Look, MaterialKind, PrefabRegistry, PrefabSpawn, SceneError,
    ShapeDef, ShapeKind, SpawnedBody, ValidationError, Watched, WatchedLevel,
    install_level_from_content_with_prefabs, installed, move_entry, spawn_shape,
};
use engine_world::{NetId, Replication, Resource, Transform, World, WorldError};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::sim::GRAVITY_MPS2;

/// Where the player's capsule centre spawns, in metres (above the floor, in the open).
pub const PLAYER_SPAWN_M: Vec3 = Vec3::new(0.0, 3.0, 14.0);
/// Colour of the stair step and the ramp, sRGB hex.
const SLOPE_HEX: u32 = 0xc9_c2b0;
/// Roughness of the stair step and the ramp.
const SLOPE_ROUGHNESS: f32 = 0.8;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Health data for the sample arena's sandbox placements.
pub struct HealthBlock {
    /// Hit points carried by the sample placement.
    pub hp: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Blast data for the sample arena's sandbox placements.
pub struct ExplosiveBlock {
    /// Explosion radius in metres.
    pub radius_m: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Door timing data for the sample arena's sandbox placements.
pub struct DoorBlock {
    /// Time to open the sample door in seconds.
    pub open_s: f32,
}

fn spawn_health(
    world: &mut World,
    placement: PrefabSpawn,
    block: HealthBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

fn spawn_explosive(
    world: &mut World,
    placement: PrefabSpawn,
    block: ExplosiveBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

fn spawn_door(
    world: &mut World,
    placement: PrefabSpawn,
    block: DoorBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

pub(crate) fn sample_prefab_registry() -> Result<PrefabRegistry, SceneError> {
    let mut registry = PrefabRegistry::default();
    registry.register("health", spawn_health)?;
    registry.register("explosive", spawn_explosive)?;
    registry.register("door", spawn_door)?;
    Ok(registry)
}

/// What a box is for. The simulation ignores it; the tests read it to find the crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRole {
    /// The ground.
    Floor,
    /// A boundary or test wall.
    Wall,
    /// A low step to climb. Any static box that is not a floor, wall or ramp counts as a step.
    Step,
    /// A sloped surface.
    Ramp,
    /// A dynamic crate.
    Crate,
}

/// One box, as data: the arguments of the engine's cuboid or ramp shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxSpec {
    /// World-space centre.
    pub centre: Vec3,
    /// Half extents along the box's own axes, in metres.
    pub half: Vec3,
    /// Rotation about the world Z axis, in radians.
    pub tilt_z_rad: f32,
    /// What the box is for.
    pub role: BoxRole,
    /// Whether the box is simulated (a crate) or fixed in place.
    pub dynamic: bool,
}

impl BoxSpec {
    /// A fixed, untilted box.
    #[must_use]
    pub const fn fixed(centre: Vec3, half: Vec3, role: BoxRole) -> Self {
        Self {
            centre,
            half,
            tilt_z_rad: 0.0,
            role,
            dynamic: false,
        }
    }

    /// The engine shape of this box. A ramp uses the ramp shape so its tilt is part of the kind;
    /// any other box is a cuboid turned about Z.
    fn to_shape(self) -> Result<ShapeDef, SceneError> {
        let half = self.half.to_array();
        let tilt_deg = self.tilt_z_rad.to_degrees();
        let (kind, rotation_deg) = match self.role {
            BoxRole::Ramp => (ShapeKind::Ramp { half, tilt_deg }, [0.0; 3]),
            _ => (ShapeKind::Cuboid { half }, [0.0, 0.0, tilt_deg]),
        };
        let body = if self.dynamic {
            // Density one, as the sandbox has always used: mass equals the box volume.
            BodyDef::Dynamic {
                mass_kg: kind.volume(),
            }
        } else {
            BodyDef::Static
        };
        let look = Look {
            material: material_of(self.role),
            tint: None,
        };
        let id = AssetId::new(1).map_err(|error| SceneError::Tuning {
            file: "sandbox box".to_owned(),
            reason: error.to_string(),
        })?;
        ShapeDef::new(
            id,
            None,
            kind,
            self.centre.to_array(),
            rotation_deg,
            body,
            look,
        )
        .map_err(|reason| SceneError::Invalid {
            file: "sandbox box".to_owned(),
            error: ValidationError::new("box", reason),
        })
    }
}

/// The material a role is drawn with when a box is built in code rather than read from the level.
fn material_of(role: BoxRole) -> MaterialKind {
    match role {
        BoxRole::Floor => MaterialKind::Floor,
        BoxRole::Wall => MaterialKind::Wall,
        BoxRole::Crate => MaterialKind::Crate,
        BoxRole::Step | BoxRole::Ramp => MaterialKind::Custom {
            colour_hex: SLOPE_HEX,
            roughness: SLOPE_ROUGHNESS,
            metallic: 0.0,
        },
    }
}

/// The role of an installed shape, from its kind and look.
fn role_of(kind: ShapeKind, look: Look) -> BoxRole {
    match (kind, look.material) {
        (ShapeKind::Ramp { .. }, _) => BoxRole::Ramp,
        (_, MaterialKind::Floor) => BoxRole::Floor,
        (_, MaterialKind::Wall) => BoxRole::Wall,
        (_, MaterialKind::Crate) => BoxRole::Crate,
        _ => BoxRole::Step,
    }
}

/// The ramp whose lower end sits on the ground at `start_x`, rising towards +X at `tilt_rad`.
/// Its top surface passes through `(start_x, 0)`, so the ramp starts at the floor, not above it.
#[must_use]
pub fn ramp(start_x: f32, z: f32, tilt_rad: f32) -> BoxSpec {
    /// Half length of a ramp along its slope, in metres.
    const HALF_LENGTH_M: f32 = 3.0;
    /// Half thickness of a ramp's slab, in metres.
    const HALF_THICKNESS_M: f32 = 0.2;
    /// Half width of a ramp across the slope, in metres.
    const HALF_WIDTH_M: f32 = 2.0;
    let (sin, cos) = tilt_rad.sin_cos();
    // The top face's lower end is `centre + R(-L, T)` where R rotates by `tilt_rad`. Solving
    // `centre + R(-L, T) = (start_x, 0)` for the centre gives:
    let centre = Vec3::new(
        start_x + HALF_LENGTH_M * cos + HALF_THICKNESS_M * sin,
        HALF_LENGTH_M * sin - HALF_THICKNESS_M * cos,
        z,
    );
    BoxSpec {
        centre,
        half: Vec3::new(HALF_LENGTH_M, HALF_THICKNESS_M, HALF_WIDTH_M),
        tilt_z_rad: tilt_rad,
        role: BoxRole::Ramp,
        dynamic: false,
    }
}

/// One scene body: the entity that draws it, its physics body and what it is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneBody {
    /// The entity whose `Transform` mirrors the body.
    pub id: NetId,
    /// The physics body.
    pub body: engine_physics::BodyId,
    /// What the box is for.
    pub role: BoxRole,
}

/// Every scene body in spawn order. Its hash lists the ids, so two equal scenes hash equally.
#[derive(Debug, Default)]
pub struct Bodies {
    entries: Vec<SceneBody>,
}

/// Mounted sandbox content used for level and cooked-scene resolution.
pub struct SceneContent(pub ContentPack);

impl Resource for SceneContent {
    fn hash_into(&self, _hasher: &mut StateHasher) {}
}

impl Bodies {
    /// Appends a body. Called once per spawned box, in spawn order.
    pub fn push(&mut self, body: SceneBody) {
        self.entries.push(body);
    }

    /// The bodies in spawn order.
    #[must_use]
    pub fn entries(&self) -> &[SceneBody] {
        &self.entries
    }

    /// Moves the list out so a system can read it while it borrows the world mutably.
    /// The caller must hand it back with [`Bodies::restore`]. Taking an empty `Vec` does not
    /// allocate, and the restored buffer keeps its capacity.
    pub(crate) fn take(&mut self) -> Vec<SceneBody> {
        std::mem::take(&mut self.entries)
    }

    /// Hands back the list taken by [`Bodies::take`], or replaces the whole list.
    pub(crate) fn restore(&mut self, entries: Vec<SceneBody>) {
        self.entries = entries;
    }
}

impl Resource for Bodies {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for entry in &self.entries {
            hasher.write_u32(entry.id.index());
            hasher.write_u32(entry.id.generation());
            hasher.write_u32(entry.body.index());
            hasher.write_u32(entry.body.generation());
        }
    }
}

/// Maps a physics rejection to an invariant failure. Every value reaching here is validated
/// before use, so a rejection means the simulation itself went wrong (for example a NaN pose).
pub(crate) fn physics<T>(
    result: Result<T, PhysicsError>,
    what: &'static str,
) -> Result<T, WorldError> {
    result.map_err(|_| WorldError::Invariant(what))
}

/// Writes a translation and rotation into an entity's `Transform`, keeping its scale.
///
/// # Errors
///
/// Returns a world error when the entity is missing or the pose is not finite.
pub(crate) fn write_pose(
    world: &mut World,
    id: NetId,
    translation: Vec3,
    rotation: Quat,
) -> Result<(), WorldError> {
    let mut transform = world.get_mut::<Transform>(id)?;
    transform.set_translation(translation)?;
    transform.set_rotation(rotation)
}

/// Spawns one box built in code and records it in [`Bodies`].
///
/// # Errors
///
/// Returns a scene error when the box is invalid, the physics world or [`Bodies`] is missing, or
/// the physics world rejects the shape.
pub fn spawn_box(world: &mut World, spec: BoxSpec) -> Result<(), SceneError> {
    let shape = spec.to_shape()?;
    let spawned = spawn_shape(world, &shape)?;
    world.resource_mut::<Bodies>()?.push(SceneBody {
        id: spawned.id,
        body: spawned.body,
        role: spec.role,
    });
    Ok(())
}

/// The sandbox's gravity check: the character's jump and fall use [`GRAVITY_MPS2`], so a level
/// with any other gravity would move the character wrongly.
fn check_gravity(file: &str, level: &Level) -> Result<(), SceneError> {
    if level.gravity() == [0.0, -GRAVITY_MPS2, 0.0] {
        Ok(())
    } else {
        Err(SceneError::Tuning {
            file: file.to_owned(),
            reason: format!(
                "gravity {:?} must be (0.0, -{GRAVITY_MPS2}, 0.0), the value the character's jump uses",
                level.gravity()
            ),
        })
    }
}

/// Loads the sandbox level from the mounted pack and applies the sandbox's own check. This is the only way
/// the sandbox loads a level, so a level that passes it is always usable.
///
/// # Errors
///
/// Returns the scene error from reading or parsing the level, or the gravity rejection.
pub fn load_sandbox_level(content: &ContentPack) -> Result<Level, SceneError> {
    const PATH: &str = "game/levels/sandbox.ron";
    let path = AssetPath::new(PATH).map_err(|error| SceneError::Tuning {
        file: PATH.to_owned(),
        reason: error.to_string(),
    })?;
    let text = content
        .load_text(&path)
        .map_err(|error| SceneError::Tuning {
            file: PATH.to_owned(),
            reason: error.to_string(),
        })?;
    let level = Level::parse(text, PATH)?;
    check_gravity(PATH, &level)?;
    Ok(level)
}

/// Installs `level` in place of the running scene and rebuilds [`Bodies`] from it. Requires the
/// physics world, the registered components and [`Bodies`] to be present.
///
/// # Errors
///
/// Returns the first error from the engine's install. The level was validated when it was loaded,
/// so an error here means the world itself is missing something.
pub fn install_scene(world: &mut World, level: &Level) -> Result<(), SceneError> {
    let content = world.resource::<SceneContent>()?.0.clone();
    if !level.prefabs().is_empty() {
        if world.registry().id_of::<HealthBlock>().is_none() {
            world.register::<HealthBlock>(Replication::ServerOnly)?;
        }
        if world.registry().id_of::<ExplosiveBlock>().is_none() {
            world.register::<ExplosiveBlock>(Replication::ServerOnly)?;
        }
        if world.registry().id_of::<DoorBlock>().is_none() {
            world.register::<DoorBlock>(Replication::ServerOnly)?;
        }
    }
    let registry = sample_prefab_registry()?;
    install_level_from_content_with_prefabs(
        world,
        level,
        &content,
        |_, _| engine_scene::ColliderMetadata::default(),
        &registry,
    )?;
    let entries: Vec<SceneBody> = installed(world)?
        .bodies()
        .iter()
        .map(|spawned: &SpawnedBody| SceneBody {
            id: spawned.id,
            body: spawned.body,
            role: role_of(spawned.kind, spawned.look),
        })
        .collect();
    world.resource_mut::<Bodies>()?.restore(entries);
    Ok(())
}

/// Applies one authored layout to its canonical spawned movers and their physics bodies.
///
/// # Errors
///
/// Returns an error when a layout mover has no installed entry or its transform is invalid.
pub fn apply_layout(world: &mut World, layout: &LayoutDef) -> Result<(), SceneError> {
    let moves = layout
        .mover_poses
        .iter()
        .map(|(source_id, pose)| {
            let entry = installed(world)?
                .entries()
                .get(source_id)
                .copied()
                .ok_or_else(|| SceneError::Tuning {
                    file: "sandbox layout".to_owned(),
                    reason: format!(
                        "layout {:?} mover {source_id:?} has no installed entry",
                        layout.name
                    ),
                })?;
            let [yaw, pitch, roll] = pose.rotation_deg.map(f32::to_radians);
            let transform = Transform::new(
                Vec3::from_array(pose.translation),
                Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll),
                Vec3::from_array(pose.scale),
            )?;
            Ok((entry, transform))
        })
        .collect::<Result<Vec<_>, SceneError>>()?;
    for (entry, transform) in moves {
        move_entry(world, entry, transform)?;
    }
    Ok(())
}

/// Loads and installs the level once from the current pack.
///
/// # Errors
///
/// Returns the load error, the install error, or the world error when the watch cannot be stored.
pub fn start_level(world: &mut World) -> Result<(), SceneError> {
    let pack = world.resource::<SceneContent>()?.0.clone();
    let level = load_sandbox_level(&pack)?;
    install_scene(world, &level)?;
    let path = AssetPath::new("game/levels/sandbox.ron").map_err(|error| SceneError::Tuning {
        file: "game/levels/sandbox.ron".to_owned(),
        reason: error.to_string(),
    })?;
    world.insert_resource(WatchedLevel::new(Watched::level(path, &pack)?))?;
    Ok(())
}

/// Game-owned prefab and collider policy used when the engine applies a live level edit.
pub fn live_scene_content(content: &ContentPack) -> Result<engine_scene::SceneContent, SceneError> {
    Ok(engine_scene::SceneContent::new(
        content.clone(),
        sample_prefab_registry()?,
        |_, _| engine_scene::ColliderMetadata::default(),
    ))
}

/// Rebuilds the sandbox's body-role index from the canonical entries after a live diff.
pub fn refresh_live_scene(world: &mut World, level: &Level) -> Result<(), SceneError> {
    let spawned = installed(world)?;
    let entries = level
        .shapes()
        .iter()
        .filter_map(|shape| {
            let entry = spawned.entries().get(&shape.id())?;
            let body = entry.body?;
            world.is_alive(entry.id).then_some(SceneBody {
                id: entry.id,
                body,
                role: role_of(shape.kind(), shape.look()),
            })
        })
        .collect();
    world.resource_mut::<Bodies>()?.restore(entries);
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use engine_physics::{PhysicsConfig, PhysicsWorld};
    use engine_scene::register_components;

    fn scene_world() -> World {
        let mut world = World::new(11).expect("world");
        let config =
            PhysicsConfig::new(1.0 / 60.0, Vec3::new(0.0, -GRAVITY_MPS2, 0.0)).expect("config");
        world
            .insert_resource(PhysicsWorld::new(config))
            .expect("physics");
        world.insert_resource(Bodies::default()).expect("bodies");
        world
            .insert_resource(SceneContent(
                crate::test_support::pack().expect("test pack"),
            ))
            .expect("content");
        register_components(&mut world).expect("components");
        world
    }

    #[test]
    fn packed_level_has_the_sandbox_shapes_in_order() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack()?;
        let level = load_sandbox_level(&content).expect("packed level");
        let shapes = level.shapes();
        assert_eq!(shapes.len(), 1 + 4 + 1 + 1 + 18);
        assert_eq!(shapes[0].name(), Some("floor"));
        let crates = shapes
            .iter()
            .filter(|shape| shape.look().material == MaterialKind::Crate)
            .count();
        assert_eq!(crates, 18);
        assert!(
            shapes
                .iter()
                .filter(|shape| shape.look().material == MaterialKind::Crate)
                .all(|shape| matches!(shape.body(), BodyDef::Dynamic { .. }))
        );
        Ok(())
    }

    #[test]
    fn a_level_with_other_gravity_is_rejected_by_name() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack()?;
        let path = AssetPath::new("game/levels/sandbox.ron").expect("path");
        let text = content
            .load_text(&path)
            .expect("packed level")
            .replace("-9.81", "-1.62");
        let level = Level::parse(&text, "moon.ron").expect("parses");
        let error = check_gravity("moon.ron", &level).expect_err("gravity rejected");
        assert!(error.to_string().contains("moon.ron"), "{error}");
        Ok(())
    }

    #[test]
    fn spawn_point_is_above_the_floor_top() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack()?;
        let level = load_sandbox_level(&content).expect("level");
        let floor = &level.shapes()[0];
        let half_y = match floor.kind() {
            ShapeKind::Cuboid { half } => half[1],
            _ => f32::NAN,
        };
        let floor_top = floor.translation()[1] + half_y;
        let feet = PLAYER_SPAWN_M.y - crate::sim::CENTRE_ABOVE_FEET_M;
        assert!(
            feet > floor_top,
            "feet {feet} at or below the floor top {floor_top}"
        );
        Ok(())
    }

    #[test]
    fn ramp_lower_end_of_the_top_face_sits_on_the_ground() {
        let spec = ramp(3.0, -8.0, 20.0_f32.to_radians());
        let rotation = Quat::from_rotation_z(spec.tilt_z_rad);
        let lower_top = spec.centre + rotation * Vec3::new(-3.0, 0.2, 0.0);
        assert!(
            (lower_top.x - 3.0).abs() < 1e-4,
            "lower end at x {}",
            lower_top.x
        );
        assert!(lower_top.y.abs() < 1e-4, "lower end at y {}", lower_top.y);
    }

    #[test]
    fn installing_the_embedded_level_fills_bodies_with_roles() {
        let mut world = scene_world();
        let content = crate::test_support::pack().expect("test pack");
        let level = load_sandbox_level(&content).expect("level");
        install_scene(&mut world, &level).expect("install");
        let bodies = world.resource::<Bodies>().expect("bodies");
        assert_eq!(bodies.entries().len(), 25);
        assert_eq!(bodies.entries()[0].role, BoxRole::Floor);
        assert_eq!(bodies.entries()[5].role, BoxRole::Step);
        assert_eq!(bodies.entries()[6].role, BoxRole::Ramp);
        let crates = bodies
            .entries()
            .iter()
            .filter(|entry| entry.role == BoxRole::Crate)
            .count();
        assert_eq!(crates, 18);
        assert_eq!(world.len(), 25);
    }

    #[test]
    fn spawn_box_records_a_role_and_adds_one_body() {
        let mut world = scene_world();
        let wall = BoxSpec::fixed(Vec3::Y, Vec3::ONE, BoxRole::Wall);
        spawn_box(&mut world, wall).expect("spawn");
        let bodies = world.resource::<Bodies>().expect("bodies");
        assert_eq!(bodies.entries().len(), 1);
        assert_eq!(bodies.entries()[0].role, BoxRole::Wall);
    }
}
