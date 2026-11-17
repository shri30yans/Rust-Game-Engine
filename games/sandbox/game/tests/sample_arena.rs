//! Loads the Blender-generated arena through the sandbox's normal scene installer.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use engine_assets::AssetPath;
use engine_scene::Level;
use engine_scene::installed;
use sandbox::scene::install_scene;

#[test]
fn sample_arena_loads_with_game_registered_prefabs_and_cooked_instances() {
    let mut app = common::new_app();
    let path = AssetPath::new("content/cooked/sample_arena.level.ron").expect("path");
    let content = common::content_pack();
    let text = content.load_text(&path).expect("sample level in pack");
    let sample = Level::parse(text, path.as_str()).expect("generated sample level validates");
    install_scene(app.world_mut(), &sample).expect("sandbox installs generated arena");
    let spawned = installed(app.world()).expect("sample level is installed");
    assert_eq!(spawned.instances().len(), sample.instances().len());
    assert_eq!(spawned.prefabs().len(), sample.prefabs().len());
}

#[test]
fn applying_lockdown_moves_the_canonical_mover_and_its_body_together() {
    use engine_physics::PhysicsWorld;
    use engine_world::Transform;
    use glam::Vec3;
    use sandbox::scene::apply_layout;

    let mut app = common::new_app();
    let content = common::content_pack();
    let sample_path = AssetPath::new("content/cooked/sample_arena.level.ron").expect("path");
    let sample = Level::parse(
        content
            .load_text(&sample_path)
            .expect("sample level in pack"),
        sample_path.as_str(),
    )
    .expect("generated sample level validates");
    install_scene(app.world_mut(), &sample).expect("sandbox installs generated arena");
    let lockdown = sample
        .layouts()
        .iter()
        .find(|layout| layout.name == "lockdown")
        .expect("sample has lockdown layout");
    let (mover_id, expected_pose) = lockdown
        .mover_poses
        .iter()
        .next()
        .expect("lockdown has a mover pose");
    let entry = installed(app.world())
        .expect("sample level is installed")
        .entries()
        .get(mover_id)
        .copied()
        .expect("layout mover is canonically spawned");
    let body = entry.body.expect("layout mover has a physics body");
    let before = *app
        .world()
        .get::<Transform>(entry.id)
        .expect("layout mover transform");

    apply_layout(app.world_mut(), lockdown).expect("apply lockdown layout");

    let transform = *app
        .world()
        .get::<Transform>(entry.id)
        .expect("moved layout transform");
    assert_ne!(transform, before, "lockdown changes the mover pose");
    assert_eq!(
        transform.translation(),
        Vec3::from_array(expected_pose.translation),
        "entity uses the selected layout translation"
    );
    let body_state = app
        .world()
        .resource::<PhysicsWorld>()
        .expect("physics")
        .body_state(body)
        .expect("mover body");
    assert_eq!(
        body_state.translation,
        transform.translation(),
        "body translation follows the entity transform"
    );
    assert_eq!(
        body_state.rotation,
        transform.rotation(),
        "body rotation follows the entity transform"
    );
}
