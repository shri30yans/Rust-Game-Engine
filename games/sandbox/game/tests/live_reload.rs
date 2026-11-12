//! Live reload at startup: simulation setup loads the level once and starts the watch from that
//! load, so the first poll finds nothing new and the running bodies keep their state. A second
//! install would rebuild every body at its spawn pose, which the test would see after the ticks.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::App;
use engine_physics::{BodyState, PhysicsWorld};
use sandbox::SandboxGame;
use sandbox::scene::Bodies;

/// The state of every scene body, in spawn order.
fn body_states(app: &App<SandboxGame>) -> Vec<BodyState> {
    let physics = app.world().resource::<PhysicsWorld>().expect("physics");
    app.world()
        .resource::<Bodies>()
        .expect("bodies")
        .entries()
        .iter()
        .map(|entry| physics.body_state(entry.body).expect("body state"))
        .collect()
}

#[test]
fn packed_startup_builds_the_scene_once() {
    let mut app = common::new_app();
    common::idle(&mut app, 30);
    let before = body_states(&app);
    assert!(!before.is_empty(), "the scene has bodies");
    assert_eq!(
        body_states(&app),
        before,
        "idle ticks preserve packed scene bodies"
    );
}
