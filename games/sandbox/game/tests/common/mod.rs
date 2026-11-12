//! Shared helpers for the scripted headless tests: an app with one job worker, a command
//! runner, and read-only views of the character, the crates and the physics world.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(dead_code)]

use app::GameEntry;
use app::{App, ViewAngles};
use engine_assets::{ContentPack, PackReader};
use engine_physics::{CharacterState, ConvexShape, PhysicsWorld, QueryFilter};
use glam::{Quat, Vec2, Vec3};
use platform::DeviceProfile;
use sandbox::scene::{Bodies, BoxRole};
use sandbox::sim::{CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M, CENTRE_ABOVE_FEET_M, SKIN_WIDTH_M};
use sandbox::{
    Axis, PlayerCommand, SandboxGame, app_config, place_player, player_body, player_state,
    set_command,
};

/// Yaw that faces +X (forward = (-sin yaw, 0, -cos yaw) = (1, 0, 0)).
pub const FACING_PLUS_X: f32 = -std::f32::consts::FRAC_PI_2;

/// An app with one job worker, so the tests start quickly and deterministically.
pub fn new_app() -> App<SandboxGame> {
    let config = app_config(Some(1)).expect("sandbox config");
    let profile = DeviceProfile::server().expect("device profile");
    let stream = platform::ContentSource::headless()
        .open_content(sandbox::SandboxEntry::CONTENT.as_str())
        .expect("sandbox content stream");
    let content = PackReader::read(stream).expect("sandbox content pack");
    App::new(SandboxGame::from_content(&content), &(), &config, &profile).expect("sandbox app")
}

/// Opens the committed sandbox pack through the headless content source.
pub fn content_pack() -> ContentPack {
    let stream = platform::ContentSource::headless()
        .open_content(sandbox::SandboxEntry::CONTENT.as_str())
        .expect("sandbox content stream");
    PackReader::read(stream).expect("sandbox content pack")
}

/// The view the scripted tests drive: level, facing +X. Built through the validated constructor,
/// as the engine's mouse look would build it.
pub fn facing_plus_x() -> ViewAngles {
    ViewAngles::new(FACING_PLUS_X, 0.0).expect("finite view")
}

/// Sets the command for one tick and steps the app by that tick.
pub fn tick(app: &mut App<SandboxGame>, command: PlayerCommand) {
    set_command(app.world_mut(), command).expect("set command");
    app.step().expect("step");
}

/// No movement and no jump, facing +X.
pub fn idle_command() -> PlayerCommand {
    PlayerCommand::new(Axis::Zero, Axis::Zero, false, facing_plus_x())
}

/// Steps `ticks` ticks with no input, facing +X.
pub fn idle(app: &mut App<SandboxGame>, ticks: u32) {
    for _ in 0..ticks {
        tick(app, idle_command());
    }
}

/// Steps `ticks` ticks with the same command.
pub fn hold(app: &mut App<SandboxGame>, ticks: u32, command: PlayerCommand) {
    for _ in 0..ticks {
        tick(app, command);
    }
}

/// Full forward input, facing +X, no jump.
pub fn walk_forward() -> PlayerCommand {
    PlayerCommand::new(Axis::Zero, Axis::Positive, false, facing_plus_x())
}

/// A single jump press facing +X: the jump flag for one tick.
pub fn jump_once() -> PlayerCommand {
    PlayerCommand::new(Axis::Zero, Axis::Zero, true, facing_plus_x())
}

/// Places the character at `position`, facing `yaw`, at rest.
pub fn place(app: &mut App<SandboxGame>, position: Vec3, yaw: f32) {
    place_player(app.world_mut(), position, yaw).expect("place player");
}

/// The character's state after the last tick.
pub fn state(app: &App<SandboxGame>) -> CharacterState {
    player_state(app.world()).expect("player state")
}

/// The character's capsule centre.
pub fn centre(app: &App<SandboxGame>) -> Vec3 {
    state(app).position()
}

/// The height of the character's lowest point above the floor top (y = 0).
pub fn feet_height(app: &App<SandboxGame>) -> f32 {
    centre(app).y - CENTRE_ABOVE_FEET_M
}

/// The character's horizontal (XZ) distance from `start`.
pub fn horizontal_distance(from: Vec3, to: Vec3) -> f32 {
    Vec2::new(to.x - from.x, to.z - from.z).length()
}

/// The linear speed of every crate, in spawn order.
pub fn crate_speeds(app: &App<SandboxGame>) -> Vec<f32> {
    let physics = app.world().resource::<PhysicsWorld>().expect("physics");
    app.world()
        .resource::<Bodies>()
        .expect("bodies")
        .entries()
        .iter()
        .filter(|entry| entry.role == BoxRole::Crate)
        .map(|entry| {
            physics
                .body_state(entry.body)
                .expect("crate body")
                .linear_velocity
                .length()
        })
        .collect()
}

/// Whether the character's capsule, inset by the skin width, overlaps any collider in the world
/// other than the player's own body. The engine answers the query, so every scene box (walls,
/// steps, ramps, crates) counts and the test does not depend on the scene's layout.
pub fn capsule_penetrates_world(app: &App<SandboxGame>) -> bool {
    let physics = app.world().resource::<PhysicsWorld>().expect("physics");
    let player = player_body(app.world()).expect("player body");
    let capsule = ConvexShape::capsule(CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M - SKIN_WIDTH_M)
        .expect("inset capsule");
    physics
        .intersects_shape(
            capsule,
            centre(app),
            Quat::IDENTITY,
            QueryFilter::default().excluding_body(player),
        )
        .expect("intersection query")
}
