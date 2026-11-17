//! Headless play over the Blender-generated arena and its Director layout cue.

mod common;

use app::{App, AssetUploader, Game, LiveScene};
use common::{Arena, TestResult};
use engine_assets::AssetStore;
use engine_scene::{Level, installed};
use engine_world::Transform;
use imagine::scene::{ArenaData, activate_layout_cue, level_from_content};
use imagine::sim::ImagineSettings;
use imagine::{ImagineGame, app_config};
use jobs::JobPool;
use platform::DeviceProfile;

const CUE_TRANSITION_TICKS: u32 = 76;

fn sample_settings() -> ImagineSettings {
    ImagineSettings {
        arena_id: 1,
        ..ImagineSettings::default()
    }
}

fn run_sample() -> Result<(u64, [f32; 3]), Box<dyn std::error::Error>> {
    let content = common::content_pack()?;
    let mut arena = Arena::with_settings(7, &sample_settings())?;
    arena.run(600)?;
    let level: Level = level_from_content(&content, 1)?;
    let mover_id = level
        .layouts()
        .first()
        .and_then(|layout| layout.mover_poses.keys().next())
        .copied()
        .ok_or("sample arena has no authored mover")?;
    activate_layout_cue(&mut arena.world, "lockdown")?;
    arena.run(CUE_TRANSITION_TICKS)?;
    assert_eq!(
        arena.world.resource::<ArenaData>()?.current_layout(),
        Some("lockdown")
    );
    let entity = installed(&arena.world)?
        .entries()
        .get(&mover_id)
        .ok_or("mover instance was not spawned")?
        .id;
    let transform = arena.world.get::<Transform>(entity)?;
    let expected = level
        .layouts()
        .get(1)
        .and_then(|layout| layout.mover_poses.get(&mover_id))
        .ok_or("lockdown layout has no mover pose")?;
    let translation = transform.translation().to_array();
    for (actual, expected) in translation.into_iter().zip(expected.translation) {
        assert!(
            (actual - expected).abs() < 0.001,
            "mover at {translation:?}"
        );
    }
    Ok((arena.world.state_hash()?, translation))
}

#[test]
fn generated_arena_runs_six_hundred_ticks_and_switches_layout_deterministically() -> TestResult {
    let first = run_sample()?;
    let second = run_sample()?;
    assert_eq!(first, second, "sample play hash or mover pose diverged");
    assert_eq!(
        first.1,
        level_from_content(&common::content_pack()?, 1)?
            .layouts()
            .get(1)
            .ok_or("lockdown layout missing")?
            .mover_poses
            .values()
            .next()
            .ok_or("lockdown mover pose missing")?
            .translation
    );
    Ok(())
}

#[test]
fn started_imagine_game_uses_the_canonical_live_diff_applier() -> TestResult {
    let settings = ImagineSettings::default();
    let config = app_config(Some(1))?;
    let profile = DeviceProfile::server()?;
    let content_pack = common::content_pack()?;
    let game = ImagineGame::from_content(&content_pack);
    let content = game.scene_content()?;
    let mut app = App::new(game, &settings, &config, &profile)?;
    let mut live = LiveScene::new();
    let mut store = AssetStore::new();
    let uploader = AssetUploader::new();
    let pool = JobPool::new(0)?;

    assert_eq!(
        live.poll(app.world_mut(), &content, &mut store, &uploader, &pool)?,
        None
    );
    Ok(())
}
